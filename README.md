# clip

One paste buffer shared by a browser, a laptop, and an agent. Every write replaces the previous value and disappears after 20 seconds.

Open the app. It creates an unguessable room in the URL; share that exact URL with the other device or agent.

```sh
# read
./bin/clip 'ROOM_URL'

# write
pbpaste | ./bin/clip 'ROOM_URL'

# copy into the macOS clipboard
./bin/clip 'ROOM_URL' | pbcopy

# clear
./bin/clip 'ROOM_URL' clear
```

`CLIP_ROOM=ROOM_URL` can replace the first argument. The HTTP API is `GET`, `PUT`, and `DELETE /api/clip?k=ROOM_KEY`; `PUT` accepts plain text up to 256 KiB.
