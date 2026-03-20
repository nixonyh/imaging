# `imaging`

Backend-agnostic 2D imaging IR + recorder.

This crate is `no_std` by default (uses `alloc`); enable the `std` feature when needed.

## API shape

`imaging` has two public layers:

- `Scene`: the owned, backend-agnostic semantic IR used for recording and replay
- `PaintSink` + `Painter`: the borrowed streaming/authoring API used to stream commands directly
  into scenes, renderers, or backend-native recorders

Pre-1.0 note: the streaming surface moved from the old owned `Sink` shape to the borrowed
`PaintSink`/`Painter` model so command authoring does not have to construct owned IR payloads
up-front.
