<img src="https://github.com/user-attachments/assets/eb509032-7d25-4acb-b594-e365c3ade9ed" width="88" align="left" alt="clip icon">
# clip


A string and a socket. One in-memory paste buffer per path, shared over HTTP.

<br clear="left">

```sh
cargo run --release                 # serve :1984
echo hi | cargo run --release       # write /
cargo run --release -- get          # read /raw
cargo run --release -- follow       # stream /events
cargo run --release -- sync         # sync macOS Command+C / Command+V
```

Open `http://127.0.0.1:1984`. Rooms are paths:

```sh
curl -X PUT --data-binary 'hello' http://127.0.0.1:1984/r/team
curl http://127.0.0.1:1984/r/team/raw
curl -N http://127.0.0.1:1984/r/team/events
```

The binary accepts the same room as its last argument: `clip get team`, `clip follow team`, `clip sync team`, or `echo hi | clip set team`. `CLIP_URL` selects the server and `CLIP_ROOM` selects the room.

On macOS, leave `clip sync` running on each laptop. Copy normally with Command+C; the other synced laptops receive it in their system clipboard and can paste it with Command+V. The bridge uses the built-in `pbpaste` and `pbcopy` commands—still no dependencies.

There is no database and no history. Restarting the process clears every room.
