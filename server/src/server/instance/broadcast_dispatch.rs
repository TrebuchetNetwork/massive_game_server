use super::*;

impl MassiveGameServer {
    pub(super) async fn process_client_broadcast(
        peer_id_str: &str,
        client_info: &ClientInfo,
        shared_data: &SharedBroadcastData,
        server: &Arc<MassiveGameServer>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let frame = server.frame_counter.load(AtomicOrdering::Relaxed);

        trace!(
            "[Frame {}] Starting broadcast for client {}",
            frame,
            peer_id_str
        );

        let player_id_arc = server.player_manager.id_pool.get_or_create(peer_id_str);
        let player_exists =
            Self::lookup_player_state_from_shared(shared_data, &player_id_arc).is_some();

        if !player_exists {
            trace!(
                "[Frame {}] Player {} absent from shared snapshot, deferring broadcast.",
                frame,
                peer_id_str
            );
            return Ok(());
        }

        if client_info.needs_initial_state {
            server.ensure_join_trace(peer_id_str, client_info.data_channel.is_open());
        }

        if client_info.needs_initial_state && !client_info.data_channel.is_open() {
            trace!(
                "[Frame {}] Client {} data channel not open yet, deferring initial state build.",
                frame,
                peer_id_str
            );
            return Ok(());
        }

        let existing_client_state = server.client_states_map.read().get(peer_id_str).cloned();
        let mut client_state_for_delta: Option<ClientState> = None;
        let mut last_chat_message_seq_sent = existing_client_state
            .as_ref()
            .map(|state| state.last_chat_message_seq_sent)
            .unwrap_or_default();
        let state_result = if client_info.needs_initial_state {
            let cached_initial_state = existing_client_state
                .as_ref()
                .and_then(|state| state.pending_initial_state_chunks.front().cloned());

            if let Some(cached_bytes) = cached_initial_state {
                trace!(
                    "[Frame {}] Reusing cached initial state chunk for {} ({} bytes)",
                    frame,
                    peer_id_str,
                    cached_bytes.len()
                );
                Ok(cached_bytes)
            } else {
                server.mark_join_build_start(peer_id_str);
                trace!(
                    "[Frame {}] Building initial state sequence for {}",
                    frame,
                    peer_id_str
                );
                let initial_result =
                    server.build_initial_state_sequence_optimized(peer_id_str, shared_data);
                if let Ok(initial_chunks) = initial_result.as_ref() {
                    server.mark_join_build_done(peer_id_str);
                    if let Some(first_chunk) = initial_chunks.front() {
                        let mut client_states = server.client_states_map.write();
                        let state_entry = client_states.entry(peer_id_str.to_string()).or_default();
                        state_entry.pending_initial_state_chunks = initial_chunks.clone();
                        state_entry.pending_initial_state_bytes = Some(first_chunk.clone());
                        state_entry.known_walls_sent = false;
                    }
                }
                match initial_result {
                    Ok(initial_chunks) => initial_chunks.front().cloned().ok_or_else(
                        || -> Box<dyn std::error::Error + Send + Sync> {
                            "initial state chunk sequence is empty".into()
                        },
                    ),
                    Err(err) => Err(err),
                }
            }
        } else {
            trace!("[Frame {}] Building delta state for {}", frame, peer_id_str);
            let mut client_state_snapshot = existing_client_state.unwrap_or_else(|| {
                debug!(
                    "[Frame {}] ClientState not found for {} during delta build, using default.",
                    server.frame_counter.load(AtomicOrdering::Relaxed),
                    peer_id_str
                );
                ClientState::default()
            });
            let delta_result = server.build_delta_state_optimized(
                peer_id_str,
                &mut client_state_snapshot,
                shared_data,
            );
            last_chat_message_seq_sent = client_state_snapshot.last_chat_message_seq_sent;
            client_state_for_delta = Some(client_state_snapshot);
            delta_result
        };

        let bytes_to_send = match state_result {
            Ok(b) => {
                trace!(
                    "[Frame {}] State built successfully for {} ({} bytes)",
                    frame,
                    peer_id_str,
                    b.len()
                );
                b
            }
            Err(_e) => {
                error!(
                    "[Frame {}] Failed to build state for {}: {:?}",
                    frame, peer_id_str, _e
                );
                return Err(format!("Failed to build state for client {}", peer_id_str).into());
            }
        };

        trace!(
            "[Frame {}] Prepared state payload {} bytes for client {}",
            frame,
            bytes_to_send.len(),
            peer_id_str
        );

        let pending_chat_packets =
            collect_pending_chat_packets(last_chat_message_seq_sent, &shared_data.chat_packets);
        let pending_direct_packets = server.drain_direct_packets_for_peer(peer_id_str, 8);
        let mut outbound_packets: Vec<Bytes> =
            Vec::with_capacity(1 + pending_chat_packets.len() + pending_direct_packets.len());
        outbound_packets.push(bytes_to_send.clone());
        outbound_packets.extend(
            pending_chat_packets
                .iter()
                .map(|packet| packet.bytes.clone()),
        );
        outbound_packets.extend(pending_direct_packets);

        const DELTA_SEND_TIMEOUT_MS: u64 = 50;
        const INITIAL_SEND_TIMEOUT_MS: u64 = 200;
        const INITIAL_SEND_TIMEOUT_TAIL_MS: u64 = 320;
        const INITIAL_SEND_TIMEOUT_AGGRESSIVE_TAIL_MS: u64 = 420;
        const INITIAL_SEND_TIMEOUT_EXTREME_TAIL_MS: u64 = 540;
        let base_send_timeout_ms = if client_info.needs_initial_state {
            if shared_data.extreme_tail_join_mode {
                INITIAL_SEND_TIMEOUT_EXTREME_TAIL_MS
            } else if shared_data.aggressive_tail_join_mode {
                INITIAL_SEND_TIMEOUT_AGGRESSIVE_TAIL_MS
            } else if shared_data.tail_join_mode {
                INITIAL_SEND_TIMEOUT_TAIL_MS
            } else {
                INITIAL_SEND_TIMEOUT_MS
            }
        } else {
            DELTA_SEND_TIMEOUT_MS
        };
        let send_timeout_ms = base_send_timeout_ms.saturating_add(
            ((outbound_packets.len().saturating_sub(1) as u64) * 12).min(INITIAL_SEND_TIMEOUT_MS),
        );

        if client_info.needs_initial_state {
            server.mark_join_send_start(peer_id_str);
        }
        let sent_packets = server
            .send_packet_batch_optimized(
                &client_info.data_channel,
                &outbound_packets,
                send_timeout_ms,
            )
            .await;
        let send_succeeded = sent_packets > 0;
        let sent_chat_packets_count = sent_packets
            .saturating_sub(1)
            .min(pending_chat_packets.len());

        let mut final_chat_message_seq_sent = last_chat_message_seq_sent;
        if sent_chat_packets_count > 0 {
            for packet in pending_chat_packets.iter().take(sent_chat_packets_count) {
                if packet.seq > final_chat_message_seq_sent {
                    final_chat_message_seq_sent = packet.seq;
                }
            }
        }
        if sent_chat_packets_count < pending_chat_packets.len() {
            final_chat_message_seq_sent = server
                .send_chat_messages_optimized(
                    &client_info.data_channel,
                    final_chat_message_seq_sent,
                    &shared_data.chat_packets,
                )
                .await;
        }

        if !send_succeeded {
            if client_info.data_channel.is_open() {
                warn!(
                    "[Frame {}] Send failed for client {} (timeout {}ms, batch packets {}).",
                    frame,
                    peer_id_str,
                    send_timeout_ms,
                    outbound_packets.len()
                );
            } else {
                trace!(
                    "[Frame {}] Send skipped for {} because data channel is not open.",
                    frame,
                    peer_id_str
                );
            }
        } else {
            trace!(
                "[Frame {}] Sent {} packet(s) to client {} in one dispatch path.",
                frame,
                sent_packets,
                peer_id_str
            );
        }

        if client_info.needs_initial_state && !send_succeeded {
            server.mark_join_send_failure(peer_id_str);
            trace!(
                "[Frame {}] Initial state send not completed for {}, retrying on next broadcast.",
                frame,
                peer_id_str
            );
            return Ok(());
        }
        if !client_info.needs_initial_state && !send_succeeded {
            trace!(
                "[Frame {}] Delta state send did not complete for {}; preserving previous snapshot.",
                frame,
                peer_id_str
            );
            return Ok(());
        }

        let mut initial_chunks_remaining = false;
        if client_info.needs_initial_state && send_succeeded {
            let mut client_states = server.client_states_map.write();
            if let Some(state) = client_states.get_mut(peer_id_str) {
                if !state.pending_initial_state_chunks.is_empty() {
                    let _ = state.pending_initial_state_chunks.pop_front();
                }
                state.pending_initial_state_bytes =
                    state.pending_initial_state_chunks.front().cloned();
                initial_chunks_remaining = !state.pending_initial_state_chunks.is_empty();
            }

            if initial_chunks_remaining {
                trace!(
                    "[Frame {}] Initial state chunk delivered for {}. Remaining chunks pending.",
                    frame,
                    peer_id_str
                );
                return Ok(());
            }
            server.mark_join_send_done(peer_id_str);
        }

        trace!(
            "[Frame {}] Updating client state for {}",
            frame,
            peer_id_str
        );
        if client_info.needs_initial_state {
            server.update_client_state_after_initial(
                peer_id_str,
                shared_data,
                final_chat_message_seq_sent,
            );
        } else {
            let mut client_state = client_state_for_delta.unwrap_or_default();
            client_state.last_chat_message_seq_sent = final_chat_message_seq_sent;

            server.update_client_state_after_delta(&mut client_state, &player_id_arc, shared_data);
            server.update_client_state_after_delta_with_shared(&mut client_state, shared_data);

            server
                .client_states_map
                .write()
                .insert(peer_id_str.to_string(), client_state);
        }

        trace!(
            "[Frame {}] Broadcast processing complete for client {}",
            frame,
            peer_id_str
        );
        Ok(())
    }

    pub(super) async fn process_quic_client_broadcast(
        peer_id_str: &str,
        needs_initial_state: bool,
        shared_data: &SharedBroadcastData,
        server: &Arc<MassiveGameServer>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let frame = server.frame_counter.load(AtomicOrdering::Relaxed);
        let player_id_arc = server.player_manager.id_pool.get_or_create(peer_id_str);
        let player_exists =
            Self::lookup_player_state_from_shared(shared_data, &player_id_arc).is_some();
        if !player_exists {
            return Ok(());
        }

        let existing_client_state = server.client_states_map.read().get(peer_id_str).cloned();
        let mut client_state_for_delta: Option<ClientState> = None;
        let mut last_chat_message_seq_sent = existing_client_state
            .as_ref()
            .map(|state| state.last_chat_message_seq_sent)
            .unwrap_or_default();
        let state_result = if needs_initial_state {
            let cached_initial_state = existing_client_state
                .as_ref()
                .and_then(|state| state.pending_initial_state_chunks.front().cloned());

            if let Some(cached_bytes) = cached_initial_state {
                Ok(cached_bytes)
            } else {
                let initial_result =
                    server.build_initial_state_sequence_optimized(peer_id_str, shared_data);
                if let Ok(initial_chunks) = initial_result.as_ref() {
                    if let Some(first_chunk) = initial_chunks.front() {
                        let mut client_states = server.client_states_map.write();
                        let state_entry = client_states.entry(peer_id_str.to_string()).or_default();
                        state_entry.pending_initial_state_chunks = initial_chunks.clone();
                        state_entry.pending_initial_state_bytes = Some(first_chunk.clone());
                        state_entry.known_walls_sent = false;
                    }
                }
                match initial_result {
                    Ok(initial_chunks) => initial_chunks.front().cloned().ok_or_else(
                        || -> Box<dyn std::error::Error + Send + Sync> {
                            "initial state chunk sequence is empty".into()
                        },
                    ),
                    Err(err) => Err(err),
                }
            }
        } else {
            let mut client_state_snapshot = existing_client_state.unwrap_or_default();
            let delta_result = server.build_delta_state_optimized(
                peer_id_str,
                &mut client_state_snapshot,
                shared_data,
            );
            last_chat_message_seq_sent = client_state_snapshot.last_chat_message_seq_sent;
            client_state_for_delta = Some(client_state_snapshot);
            delta_result
        };

        let bytes_to_send = match state_result {
            Ok(bytes) => bytes,
            Err(err) => {
                return Err(format!(
                    "[Frame {}] failed building QUIC payload for {}: {}",
                    frame, peer_id_str, err
                )
                .into());
            }
        };

        let pending_chat_packets =
            collect_pending_chat_packets(last_chat_message_seq_sent, &shared_data.chat_packets);
        let pending_direct_packets = server.drain_direct_packets_for_peer(peer_id_str, 8);
        let mut outbound_packets =
            Vec::with_capacity(1 + pending_chat_packets.len() + pending_direct_packets.len());
        outbound_packets.push(bytes_to_send);
        outbound_packets.extend(
            pending_chat_packets
                .iter()
                .map(|packet| packet.bytes.clone()),
        );
        outbound_packets.extend(pending_direct_packets);

        let sent_packets = send_quic_packet_batch(peer_id_str, &outbound_packets);
        let send_succeeded = sent_packets > 0;
        if !send_succeeded {
            trace!(
                "[Frame {}] QUIC send skipped/failed for {}",
                frame,
                peer_id_str
            );
            return Ok(());
        }

        let sent_chat_packets_count = sent_packets
            .saturating_sub(1)
            .min(pending_chat_packets.len());
        let mut final_chat_message_seq_sent = last_chat_message_seq_sent;
        for packet in pending_chat_packets.iter().take(sent_chat_packets_count) {
            if packet.seq > final_chat_message_seq_sent {
                final_chat_message_seq_sent = packet.seq;
            }
        }

        if needs_initial_state {
            let mut client_states = server.client_states_map.write();
            let mut chunks_remaining = false;
            if let Some(state) = client_states.get_mut(peer_id_str) {
                if !state.pending_initial_state_chunks.is_empty() {
                    let _ = state.pending_initial_state_chunks.pop_front();
                }
                state.pending_initial_state_bytes =
                    state.pending_initial_state_chunks.front().cloned();
                chunks_remaining = !state.pending_initial_state_chunks.is_empty();
            }

            if chunks_remaining {
                return Ok(());
            }

            server.update_client_state_after_initial(
                peer_id_str,
                shared_data,
                final_chat_message_seq_sent,
            );
        } else {
            let mut client_state = client_state_for_delta.unwrap_or_default();
            client_state.last_chat_message_seq_sent = final_chat_message_seq_sent;
            server.update_client_state_after_delta(&mut client_state, &player_id_arc, shared_data);
            server.update_client_state_after_delta_with_shared(&mut client_state, shared_data);
            server
                .client_states_map
                .write()
                .insert(peer_id_str.to_string(), client_state);
        }

        Ok(())
    }
}
