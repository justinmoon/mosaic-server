Step 5 — Fetch Path & CLI Demo

Objective
- Support a manual and automated user story: run the server, publish via the client, and fetch the record back via GET.

Artifacts
- GET handling in the server using the LMDB store, returning Record + QueryClosed frames.
- Updated example client to handshake, publish, and fetch.
- Integration test `tests/publish_smoke.rs` covering publish + get + duplicate semantics.
- Shell script `scripts/publish_fetch_cli.sh` exercising the CLI roundtrip via cargo examples.

Notes
- GET replies with `QueryClosed(NotFound)` when no references match, and `QueryClosed(Invalid)` if HELLO/HELLO ACK hasn’t completed. These behaviors are documented in `SPEC_NOTES.md` for future review.
- CLI script uses `MOSAIC_DATA_DIR` to isolate data and assumes port 8081 per examples.
