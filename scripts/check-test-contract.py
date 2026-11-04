"""Fail CI if release-critical regressions disappear from Cargo's test inventory."""
import subprocess

REQUIRED = {
    "receiver_requires_consent_before_upload",
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
}

if __name__ == "__main__":
    output = subprocess.check_output(
        ["cargo", "test", "--workspace", "--locked", "--", "--list"], text=True
    )
    found = {line.removesuffix(": test").split("::")[-1]
             for line in output.splitlines() if line.endswith(": test")}
    missing = REQUIRED - found
    if missing:
        raise SystemExit("Required regression tests missing: " + ", ".join(sorted(missing)))
    print(f"All {len(REQUIRED)} required regression tests are present.")
