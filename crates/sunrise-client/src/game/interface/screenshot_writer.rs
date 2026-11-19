//! Screenshot requests are drawn on the client; readback and PNG encoding run on workers.
use super::{Game, chat::screenshot_name};
use mod_api::ScreenshotRequestError;
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
};
use sunrise_client::gfx::{
    PendingScreenshot, ScreenshotOutcome, ScreenshotReadback, ScreenshotRequest, ScreenshotSave,
    ScreenshotScale,
};
use sunrise_client::text::Text;

pub struct ScreenshotWrite {
    save: ScreenshotSave,
    message: Option<Text>,
    chat: bool,
}

impl ScreenshotWrite {
    pub(super) fn start(
        pending: PendingScreenshot,
        destination: Option<(PathBuf, String)>,
        message: Option<Text>,
        chat: bool,
    ) -> Self {
        Self {
            save: pending.save_background(destination),
            message,
            chat,
        }
    }
}

pub(super) enum ModScreenshot {
    Reading {
        request: Option<ScreenshotRequest>,
        path: PathBuf,
        receiver: Receiver<ScreenshotOutcome>,
    },
    Writing(ScreenshotSave),
}

impl ModScreenshot {
    pub(super) fn path(&self) -> &Path {
        match self {
            Self::Reading { path, .. } => path,
            Self::Writing(save) => &save.original,
        }
    }
}

impl Game<'_> {
    pub(in crate::game) fn request_screenshot(
        &mut self,
        size: Option<[u32; 2]>,
    ) -> Result<(), ScreenshotRequestError> {
        use ScreenshotRequestError::{Busy, InvalidSize, OutputUnavailable, Unavailable};
        let renderer = self.renderer.as_ref().ok_or(Unavailable)?;
        if self.title.is_some() || self.player.is_none() || renderer.device_lost() {
            return Err(Unavailable);
        }
        if self.screenshot_sequence.is_some()
            || self.pending_screenshot.is_some()
            || !self.ui.screenshot_messages.is_empty()
            || self.ui.mod_screenshot.is_some()
            || renderer.screenshot.is_some()
        {
            return Err(Busy);
        }
        if size.is_some_and(|size| !renderer.supports_screenshot_size(size)) {
            return Err(InvalidSize);
        }
        let dir = sunrise_client::logging::player_dir().join("screenshots");
        std::fs::create_dir_all(&dir).map_err(|_| OutputUnavailable)?;
        let path = dir.join(screenshot_name(&dir, &self.ui.screenshot_messages, None));
        let (reply, receiver) = mpsc::channel();
        let request = ScreenshotRequest {
            path: path.clone(),
            size: None,
            scale: size.map_or(ScreenshotScale::Configured, ScreenshotScale::Exact),
            readback: ScreenshotReadback::Reply(reply),
        };
        self.ui.mod_screenshot = Some(ModScreenshot::Reading {
            request: Some(request),
            path,
            receiver,
        });
        Ok(())
    }

    /// Hand off the queued request before selecting this frame's aspect. A shortcut
    /// already queued owns this frame; a request from a draw hook waits for the next.
    pub(super) fn stage_mod_screenshot(&mut self) {
        if let Some(ModScreenshot::Reading { request, .. }) = &mut self.ui.mod_screenshot
            && self.title.is_none()
            && self.player.is_some()
            && self.screenshot_sequence.is_none()
            && let Some(renderer) = &mut self.renderer
            && !renderer.suspended
            && !renderer.device_lost()
            && renderer.screenshot.is_none()
            && let Some(request) = request.take()
        {
            let _ = renderer.replace_screenshot(request);
        }
    }

    /// A world or device went away before capture. Dropping the request's sender reports an
    /// interruption; an image already being read or written can finish independently.
    pub(super) fn interrupt_queued_mod_screenshot(&mut self) {
        if let Some(ModScreenshot::Reading { request, .. }) = &mut self.ui.mod_screenshot {
            request.take();
            if let Some(renderer) = &mut self.renderer {
                renderer
                    .screenshot
                    .take_if(|shot| matches!(shot.readback, ScreenshotReadback::Reply(_)));
            }
        }
    }

    pub(super) fn poll_mod_screenshot(&mut self) {
        match &mut self.ui.mod_screenshot {
            None => {}
            Some(ModScreenshot::Reading { path, receiver, .. }) => {
                let result = match receiver.try_recv() {
                    Ok(result) => result,
                    Err(TryRecvError::Empty) => return,
                    Err(TryRecvError::Disconnected) => Err("screenshot was interrupted".into()),
                };
                match result {
                    Ok(pending) => {
                        // The same guarded destination check as F2, without dispatching its hook.
                        let destination = (
                            sunrise_client::logging::player_dir().join("screenshots"),
                            path.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                        );
                        self.ui.mod_screenshot = Some(ModScreenshot::Writing(
                            pending.save_background(Some(destination)),
                        ));
                    }
                    Err(error) => {
                        let path = std::mem::take(path);
                        self.ui.mod_screenshot = None;
                        self.report_screenshot(&path, &Err(error), None);
                    }
                }
            }
            Some(ModScreenshot::Writing(save)) => {
                if let Some((path, result)) = save.try_complete() {
                    self.ui.mod_screenshot = None;
                    self.report_screenshot(&path, &result, None);
                }
            }
        }
    }

    pub(super) fn poll_screenshot_writes(&mut self) {
        let mut index = 0;
        while index < self.ui.screenshot_writes.len() {
            let Some((path, result)) = self.ui.screenshot_writes[index].save.try_complete() else {
                index += 1;
                continue;
            };
            let write = self.ui.screenshot_writes.remove(index);
            if write.chat {
                self.ui
                    .screenshot_messages
                    .retain(|path| *path != write.save.original);
                self.ui.screenshot_messages.insert(0, path);
                self.finish_screenshot(&result, write.message);
            } else if let Err(error) = result {
                log::warn!("Sunrise: couldn't save the world icon: {error}");
            }
        }
    }
}
