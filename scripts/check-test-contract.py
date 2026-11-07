"""Fail CI if release-critical regressions disappear from Cargo's test inventory."""
import subprocess

REQUIRED = {
    "receiver_requires_consent_before_upload",
    "idle_consent_expires_but_a_long_running_video_keeps_its_approval",
    "gzip_cpio_round_trip_and_rejects_traversal",
    "windows_reserved_paths_are_refused_also_on_linux",
    "gzip_checksum_and_truncation_are_not_accepted",
    "video_larger_than_old_512_mib_limit_uses_bounded_pieces",
    "huge_http_chunk_is_streamed_and_trailers_do_not_become_a_second_offer",
    "repeated_offers_before_and_during_upload_prompt_once",
    "reconnect_reuses_consent_but_different_transfer_or_size_cannot_replace_it",
    "another_peer_or_unsafe_offer_never_reuses_consent",
    "validated_archive_completes_before_the_sender_finishes_its_last_chunk",
    "corrupt_incomplete_and_unoffered_uploads_remove_parts_and_release_reservation",
    "automatic_preparation_keeps_actions_disabled_until_the_adapter_is_ready",
    "connection_updates_do_not_interrupt_a_transfer_and_a_lost_session_is_not_ready",
    "cancelled_session_cannot_replace_the_home_screen",
    "supported_chipsets_and_oem_ids_can_replace_the_original_adapter",
    "ambiguous_adapters_require_selection_but_one_attached_adapter_is_reused",
    "continuous_radio_input_does_not_starve_host_acknowledgments",
    "queue_backpressure_stops_host_reads_while_radio_keeps_receiving",
    "advertised_ht_mask_preserves_all_receive_streams",
    "receiver_declares_original_heif_and_hevc_support",
    "observed_tcp_data_allows_only_one_timely_ack_for_that_peer_and_flow",
    "unrelated_queued_packets_do_not_block_an_observed_ack",
    "ecn_does_not_disable_observed_presence_acknowledgments",
    "grouped_block_ack_requires_both_peer_and_backend_support",
    "full_size_grouped_frames_preserve_every_payload",
    "http3_streams_require_consent_preserve_files_and_finish_without_sender_fin",
}

if __name__ == "__main__":
    output = subprocess.check_output(
        ["cargo", "test", "--workspace", "--locked", "--all-features", "--", "--list"], text=True
    )
    found = {line.removesuffix(": test").split("::")[-1]
             for line in output.splitlines() if line.endswith(": test")}
    missing = REQUIRED - found
    if missing:
        raise SystemExit("Required regression tests missing: " + ", ".join(sorted(missing)))
    print(f"All {len(REQUIRED)} required regression tests are present.")
