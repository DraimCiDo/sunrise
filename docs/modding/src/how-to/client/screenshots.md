# Request a screenshot

A client mod can call `ClientHost::request_screenshot` while a world is open. For example, a mod can request a capture in response to its own key mapping:

```rust,ignore
host.request_screenshot(Some([3840, 2160]))?;
```

Pass `None` to use the player's F2 capture sizing. Pass `Some([width, height])` to draw a frame offscreen at those exact pixel dimensions and aspect ratio. This neither resizes the window nor crops or upscales the window image. An exact capture that cannot be drawn fails; it is not retried at a smaller size.

`Ok(())` means a capture was queued, not that it was saved. The game chooses an unused timestamped name under the player's `screenshots/` folder and reports success or failure in chat. The request does not take a filename, return pixels, or fire the F2-only `screenshot` hook.

Only one mod request can be outstanding across all mods, including while the PNG is being written. Handle `Busy` without retrying every frame; an already queued capture has priority, and F2 can take the frame before a mod capture draws. `Unavailable` means the world or renderer is unavailable, `InvalidSize` means an explicit size is zero or exceeds a device limit, and `OutputUnavailable` means the screenshots folder could not be opened or created. This API works in both native and wasm mods.
