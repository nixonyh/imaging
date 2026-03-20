# Borrowed Paint API Refactor Plan

## Goals

- Separate authoring/streaming from owned recording.
- Introduce a borrowed paint sink API that can stream directly into renderers or backend scene recorders.
- Keep `imaging::Scene` as the owned, backend-agnostic semantic IR.
- Preserve validation and make it work for the borrowed streaming API.
- Add a painter-style authoring layer that improves ergonomics without hiding semantics.

## Non-goals

- No new production dependencies.
- No attempt to make backend-native recordings portable across backends.
- No broad redesign of the semantic model for clips, groups, compositing, or filters.

## Execution Slices

1. Add borrowed reference payload types and a borrowed sink trait in `imaging`.
2. Rework `Scene`, replay, and validation around the borrowed sink trait while keeping the owned IR.
3. Add `Painter` and fluent authoring builders with scoped clip/group helpers.
4. Migrate backends and tests to the borrowed authoring/streaming API.
5. Update docs and run `fmt`, `clippy`, and tests before landing clean commits.

## Risks

- Lifetimes on borrowed payload types may complicate validation and replay paths.
- Backend adapters may surface places where the semantic API is still too tied to the old owned enums.
- Builder ergonomics need to work with `&mut dyn PaintSink`, not only `Scene`.
