use super::types::UserRecord;
use super::{
    ACTIVE_PHONE_HASH_PREFIX, DELETED_PHONE_HASH_PREFIX, GDPR_HASH_SALT, OTP_CODE_DIGITS,
    OTP_CODE_UPPER_BOUND,
};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

pub(super) fn generate_otp_code() -> String {
    // Rejection sampling avoids modulo bias while keeping OTP generation
    // cryptographically secure with OS entropy.
    const REJECTION_THRESHOLD: u32 = u32::MAX - (u32::MAX % OTP_CODE_UPPER_BOUND);
    loop {
        let candidate = OsRng.next_u32();
        if candidate < REJECTION_THRESHOLD {
            let value = candidate % OTP_CODE_UPPER_BOUND;
            return format!("{value:0width$}", width = OTP_CODE_DIGITS);
        }
    }
}

pub(super) fn constant_time_eq_str(left: &str, right: &str) -> bool {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    let mut diff = left_bytes.len() ^ right_bytes.len();
    let max_len = left_bytes.len().max(right_bytes.len());

    for idx in 0..max_len {
        let l = left_bytes.get(idx).copied().unwrap_or(0);
        let r = right_bytes.get(idx).copied().unwrap_or(0);
        diff |= usize::from(l ^ r);
    }

    diff == 0
}

pub(super) fn normalize_phone_number(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut had_plus = false;
    let mut digits = String::new();
    for (index, ch) in trimmed.chars().enumerate() {
        if ch == '+' {
            if index == 0 {
                had_plus = true;
                continue;
            }
            return None;
        }
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        if ch == ' ' || ch == '-' || ch == '(' || ch == ')' || ch == '.' {
            continue;
        }
        return None;
    }

    if !had_plus {
        if digits.len() == 10 {
            digits = format!("1{}", digits);
        } else if digits.len() == 11 && digits.starts_with('1') {
            // Already a North America number with country prefix.
        }
    }

    if digits.len() < 8 || digits.len() > 15 {
        return None;
    }

    Some(format!("+{}", digits))
}

pub(super) fn mask_phone_number(phone_number: &str) -> String {
    let mut digits_only = String::new();
    for ch in phone_number.chars() {
        if ch.is_ascii_digit() {
            digits_only.push(ch);
        }
    }
    if digits_only.len() <= 2 {
        // Too short to mask meaningfully; return fully masked.
        return "+***".to_owned();
    }
    let last2 = &digits_only[digits_only.len() - 2..];
    let masked_count = digits_only.len().saturating_sub(2);
    let stars: String = std::iter::repeat_n('*', masked_count).collect();
    format!("+{}{}", stars, last2)
}

pub(super) fn masked_phone_for_user(user: &UserRecord) -> String {
    if user.phone_number.starts_with(ACTIVE_PHONE_HASH_PREFIX)
        || user.phone_number.starts_with(DELETED_PHONE_HASH_PREFIX)
    {
        return masked_phone_from_last4(&user.phone_last4);
    }
    mask_phone_number(&user.phone_number)
}

pub(super) fn masked_phone_from_last4(last4: &str) -> String {
    let sanitized: String = last4.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if sanitized.is_empty() {
        return "+***".to_owned();
    }
    if sanitized.len() <= 2 {
        return format!("+***{}", sanitized);
    }
    let suffix = &sanitized[sanitized.len() - 2..];
    format!("+***{}", suffix)
}

pub(super) fn active_phone_lookup_key(phone_number: &str) -> String {
    format!(
        "{}{}",
        ACTIVE_PHONE_HASH_PREFIX,
        hash_phone_for_anonymization(phone_number)
    )
}

pub(super) fn phone_hash_from_stored_or_legacy_value(value: &str) -> String {
    if let Some(existing_hash) = value.strip_prefix(ACTIVE_PHONE_HASH_PREFIX) {
        return existing_hash.to_owned();
    }
    if let Some(existing_hash) = value.strip_prefix(DELETED_PHONE_HASH_PREFIX) {
        return existing_hash.to_owned();
    }
    hash_phone_for_anonymization(value)
}

pub(super) fn configure_gdpr_hash_salt(configured: Option<&str>) {
    let salt = configured
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(str::as_bytes)
        .map(|bytes| bytes.to_vec())
        .unwrap_or_else(|| {
            warn!(
                "MGS_GDPR_HASH_SALT is not set; using built-in compatibility salt. \
                 Configure a deployment-specific salt for production."
            );
            b"mgs-gdpr-anonymization-salt".to_vec()
        });
    let _ = GDPR_HASH_SALT.set(salt);
}

fn gdpr_hash_salt_bytes() -> &'static [u8] {
    GDPR_HASH_SALT
        .get_or_init(|| b"mgs-gdpr-anonymization-salt".to_vec())
        .as_slice()
}

/// Produce a one-way SHA-256 hash of a phone number for anonymization.
/// The result is a hex-encoded hash that cannot be reversed to the original
/// phone number but can be used to detect re-registration from the same phone.
pub(super) fn hash_phone_for_anonymization(phone_number: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(phone_number.as_bytes());
    hasher.update(gdpr_hash_salt_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}

pub(super) fn phone_last4(phone_number: &str) -> String {
    let digits: String = phone_number
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect();
    if digits.len() <= 4 {
        return digits;
    }
    digits[digits.len() - 4..].to_owned()
}

pub(super) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
