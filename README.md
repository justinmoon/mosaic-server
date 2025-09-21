mosaic-server
=============

This is a library with which you can build a Mosaic server.

See [Mosaic protocol](https://mikedilger.github.io/mosaic-spec/).

## Quick start

Run the example server (defaults to `127.0.0.1:8081` and `./mosaic-data`). Set `MOSAIC_SERVER_ADDR`
to override the bind address/port if needed:

```
cargo run --example server
```

In another terminal, publish and fetch a record via the example client (uses the same
`MOSAIC_SERVER_ADDR` default/override):

```
cargo run --example client
```

To exercise the same flow non-interactively, use the helper script (spins up the
server with an isolated data directory, runs the client, then shuts the server down):

```
./scripts/publish_fetch_cli.sh
```
