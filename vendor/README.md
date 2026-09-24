# gpui-component patch

`gpui-component` 0.5.1 is copied from crates.io with its Apache 2.0 license.
The changes in `src/text/node.rs` follow two upstream fixes:

- [Clipped Markdown list text](https://github.com/longbridge/gpui-component/pull/2158): list items and their text wrappers receive explicit width and shrink constraints.
- [Missing Markdown block spacing](https://github.com/longbridge/gpui-component/pull/2093): non-scrollable document blocks compute whether they are last individually.
- [Streaming reparse starvation](https://github.com/longbridge/gpui-kit/issues/2634): schedule a Markdown reparse after the first text change in a burst and keep the newest text, with an 80 ms interval.
- Avoid submitting unchanged Markdown to the parser on every application redraw.

The app uses this patch through `Cargo.toml` until it can move to a release
containing the fix without changing GPUI versions.
