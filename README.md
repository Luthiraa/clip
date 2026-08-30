# clip

A string and a socket. One in-memory paste buffer per path, shared over HTTP.

```sh
cargo run --release                 # serve :1984
echo hi | cargo run --release       # write /
cargo run --release -- get          # read /raw
cargo run --release -- follow       # stream /events
```

Open `http://127.0.0.1:1984`. Rooms are paths:

```sh
curl -X PUT --data-binary 'hello' http://127.0.0.1:1984/r/team
curl http://127.0.0.1:1984/r/team/raw
curl -N http://127.0.0.1:1984/r/team/events
```

The binary accepts the same room as its last argument: `clip get team`, `clip follow team`, or `echo hi | clip set team`. `CLIP_URL` selects the server and `CLIP_ROOM` selects the room.

There is no database and no history. Restarting the process clears every room.
