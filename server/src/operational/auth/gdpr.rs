use super::persistence::spawn_persist_auth_store;
use super::phone_utils::{
    hash_phone_for_anonymization, phone_hash_from_stored_or_legacy_value, unix_now,
};
use super::types::{
    AccountDeletionResult, AuthError, AuthService, CancelDeletionResult, PendingDeletion,
};
use super::{DELETED_PHONE_HASH_PREFIX, DELETION_PROCESSING_INTERVAL_SECS};
use tracing::{debug, info};

impl AuthService {
    /// Queue an account for deletion after the grace period.
    /// Returns the deletion schedule details on success.
    pub(super) fn request_account_deletion(
        &self,
        user_id: &str,
    ) -> Result<AccountDeletionResult, AuthError> {
        // Check if already pending
        if self.inner.deletion_queue.contains_key(user_id) {
            return Err(AuthError::DeletionAlreadyPending);
        }

        // Check the user exists and is not already deleted
        {
            let store = self.inner.persistent_store.read();
            match store.users.get(user_id) {
                Some(user) if user.deleted => return Err(AuthError::AccountDeleted),
                Some(_) => {}
                None => return Err(AuthError::SessionInvalid),
            }
        }

        let now = unix_now();
        let grace_seconds = self.inner.deletion_grace_period_hours.saturating_mul(3600);
        let scheduled_deletion_time = now.saturating_add(grace_seconds);

        let pending = PendingDeletion {
            user_id: user_id.to_owned(),
            requested_at: now,
            scheduled_deletion_time,
        };

        self.inner
            .deletion_queue
            .insert(user_id.to_owned(), pending.clone());

        let store_snapshot = {
            let mut store = self.inner.persistent_store.write();
            store
                .pending_deletions
                .insert(user_id.to_owned(), pending.clone());
            store.clone()
        };
        spawn_persist_auth_store(
            self.inner.store_path.clone(),
            store_snapshot,
            self.inner.clone(),
        );

        info!(
            "Account deletion queued for user_id={}, scheduled_at={}",
            user_id, scheduled_deletion_time
        );

        Ok(AccountDeletionResult {
            user_id: user_id.to_owned(),
            requested_at: now,
            scheduled_deletion_time,
            grace_period_hours: self.inner.deletion_grace_period_hours,
            message: format!(
                "Account deletion scheduled. You have {} hours to cancel. After that, your data will be permanently anonymized.",
                self.inner.deletion_grace_period_hours
            ),
        })
    }

    /// Cancel a pending account deletion within the grace period.
    pub(super) fn cancel_account_deletion(
        &self,
        user_id: &str,
    ) -> Result<CancelDeletionResult, AuthError> {
        match self.inner.deletion_queue.remove(user_id) {
            Some((_key, pending)) => {
                let store_snapshot = {
                    let mut store = self.inner.persistent_store.write();
                    store.pending_deletions.remove(user_id);
                    store.clone()
                };
                spawn_persist_auth_store(
                    self.inner.store_path.clone(),
                    store_snapshot,
                    self.inner.clone(),
                );
                info!(
                    "Account deletion cancelled for user_id={} (was scheduled for {})",
                    user_id, pending.scheduled_deletion_time
                );
                Ok(CancelDeletionResult {
                    user_id: user_id.to_owned(),
                    cancelled: true,
                    message: "Account deletion has been cancelled. Your account is safe."
                        .to_owned(),
                })
            }
            None => Err(AuthError::DeletionNotPending),
        }
    }

    /// Anonymize user data: replace PII with hashed/generic values.
    /// This is the core GDPR "right to erasure" implementation.
    pub(super) fn anonymize_user_data(&self, user_id: &str) {
        self.inner.deletion_queue.remove(user_id);
        let mut store = self.inner.persistent_store.write();

        // Extract the original phone number for removal, then mutate the user.
        let original_phone = store.users.get(user_id).map(|u| u.phone_number.clone());
        let active_phone_hash = original_phone
            .as_deref()
            .map(phone_hash_from_stored_or_legacy_value);

        // Remove the original phone->user_id mapping
        if let Some(ref phone) = original_phone {
            store.phone_to_user_id.remove(phone);
        }
        if let Some(ref phone_hash) = active_phone_hash {
            store.phone_to_user_id.remove(&format!(
                "{}{}",
                super::ACTIVE_PHONE_HASH_PREFIX,
                phone_hash
            ));
        }

        store.pending_deletions.remove(user_id);

        if let Some(user) = store.users.get_mut(user_id) {
            // Hash the phone number so we can detect re-registration
            // but can never reverse it.
            let phone_hash = active_phone_hash.clone().unwrap_or_else(|| {
                hash_phone_for_anonymization(original_phone.as_deref().unwrap_or(""))
            });
            let hash_last4 = &phone_hash[phone_hash.len().saturating_sub(4)..];

            // Store the hash in phone_number field for re-registration detection
            user.phone_number = format!("{}{}", DELETED_PHONE_HASH_PREFIX, phone_hash);
            user.phone_last4 = "0000".to_owned();
            user.display_name = format!("Deleted User #{}", hash_last4);
            user.last_game_username = None;
            user.deleted = true;
            user.updated_at = unix_now();

            // Add the hashed phone to phone_to_user_id so we can detect
            // re-registration attempts from the same phone.
            store.phone_to_user_id.insert(
                format!("{}{}", DELETED_PHONE_HASH_PREFIX, phone_hash),
                user_id.to_owned(),
            );

            info!("User data anonymized for user_id={}", user_id);
        }

        let store_snapshot = store.clone();
        drop(store);

        // Clear all session tokens for this user
        self.revoke_all_sessions_for_user(user_id);

        // Persist the anonymized store
        spawn_persist_auth_store(
            self.inner.store_path.clone(),
            store_snapshot,
            self.inner.clone(),
        );
    }

    /// Revoke all active session tokens belonging to a specific user.
    pub(super) fn revoke_all_sessions_for_user(&self, user_id: &str) {
        let tokens_to_remove: Vec<String> = self
            .inner
            .sessions
            .iter()
            .filter(|entry| entry.value().user_id == user_id)
            .map(|entry| entry.key().clone())
            .collect();

        let count = tokens_to_remove.len();
        for token in tokens_to_remove {
            self.inner.sessions.remove(&token);
        }
        if count > 0 {
            debug!("Revoked {} session token(s) for user_id={}", count, user_id);
        }
    }

    /// Process all queued deletions whose grace period has expired.
    /// Returns the number of accounts that were anonymized.
    pub fn process_pending_deletions(&self) -> usize {
        let now = unix_now();
        let mut processed = 0usize;

        // Collect user IDs that are past their grace period
        let ready: Vec<String> = self
            .inner
            .deletion_queue
            .iter()
            .filter(|entry| now >= entry.value().scheduled_deletion_time)
            .map(|entry| entry.key().clone())
            .collect();

        for user_id in &ready {
            if let Some((_key, pending)) = self.inner.deletion_queue.remove(user_id) {
                info!(
                    "Processing scheduled deletion for user_id={} (requested_at={}, scheduled_for={})",
                    pending.user_id, pending.requested_at, pending.scheduled_deletion_time
                );
                self.anonymize_user_data(user_id);
                processed += 1;
            }
        }

        if processed > 0 {
            info!(
                "Processed {} pending account deletion(s), {} remaining in queue",
                processed,
                self.inner.deletion_queue.len()
            );
        }

        processed
    }

    /// Start the background task that periodically processes queued deletions.
    /// Should be called once at server startup.
    pub fn start_deletion_processor(self) {
        let interval_secs = DELETION_PROCESSING_INTERVAL_SECS;
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_secs(interval_secs));
                // The first tick completes immediately; skip it so we don't
                // run on startup before any deletions could be queued.
                interval.tick().await;
                loop {
                    interval.tick().await;
                    let count = self.process_pending_deletions();
                    if count > 0 {
                        info!("Deletion processor: anonymized {} account(s)", count);
                    }
                }
            });
        } else {
            debug!(
                "No tokio runtime available; deletion processor not started (test environment)."
            );
        }
    }
}
