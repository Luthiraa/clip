<img src="https://github.com/user-attachments/assets/c817315d-5766-47f9-8dea-ce7722f71ac2" width="120" align="left" alt="clip icon" />

<br/>
<br/>
<br/>


# clip

One live string for people, laptops, and agents.

<a href="https://github.com/Luthiraa/clip/releases/tag/v0.2.0"><img alt="release v0.2.0" src="https://img.shields.io/badge/release-v0.2.0-111?style=flat-square"></a>

<br clear="left">

```text
PUT /         write
GET /raw      read
GET /events   live
```

```sh
cargo run --release                 # serve :1984
echo hi | cargo run --release       # write
cargo run --release -- sync         # macOS Command+C / Command+V
```

Rooms are paths: `/r/team`. Memory only, last write wins, zero dependencies.
