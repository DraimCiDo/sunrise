use super::native_entity_refs::tests::{run_fresh_cases, with_client_game};
use mod_api::{
    ClientHost, Flow, Hooks, ModEntry, RenderFramePreEvent, ScreenshotEvent, ScreenshotRequestError,
};
use std::{
    fs,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed},
    time::{Duration, Instant},
};

const EXACT: [u32; 2] = [512, 256];
static QUEUED: AtomicBool = AtomicBool::new(false);
static F2_EVENTS: AtomicUsize = AtomicUsize::new(0);

struct Capture;
#[mod_api::__subscriptions]
impl Hooks for Capture {
    fn render_frame_pre(&self, host: &mut dyn ClientHost, _: &mut RenderFramePreEvent) {
        if !QUEUED.swap(true, Relaxed) {
            host.request_screenshot(Some(EXACT)).unwrap();
            assert_eq!(
                host.request_screenshot(None),
                Err(ScreenshotRequestError::Busy)
            );
        }
    }

    fn screenshot(&self, _: &mut dyn ClientHost, _: &mut ScreenshotEvent<'_>) -> Flow {
        F2_EVENTS.fetch_add(1, Relaxed);
        Flow::Continue
    }
}

#[test]
#[ignore = "daily"]
fn a_mod_saves_one_exact_offscreen_frame_without_f2() {
    let name = "game::screenshot_request_tests::a_mod_saves_one_exact_offscreen_frame_without_f2";
    if run_fresh_cases(name, "SUNRISE_SCREENSHOT_REQUEST_CHILD", &["capture"]) {
        return;
    }
    let player = tempfile::tempdir().unwrap();
    sunrise_client::logging::set_player_dir(player.path().into());
    let dir = player.path().join("screenshots");
    let mods = vec![ModEntry::new("capture", Box::new(Capture))].into();
    mod_platform::testing::with_mods(mods, || {
        with_client_game(|game| {
            let request = |host: &mut dyn ClientHost, size| host.request_screenshot(size);
            assert_eq!(
                request(game, None),
                Err(ScreenshotRequestError::Unavailable)
            );
            let mut builder = winit::event_loop::EventLoop::builder();
            #[cfg(target_os = "linux")]
            {
                use winit::platform::x11::EventLoopBuilderExtX11;
                builder.with_x11().with_any_thread(true);
            }
            let event_loop = builder.build().unwrap();
            #[allow(deprecated)]
            let window = std::sync::Arc::new(
                event_loop
                    .create_window(
                        winit::window::Window::default_attributes()
                            .with_visible(false)
                            .with_inner_size(winit::dpi::PhysicalSize::new(320, 240)),
                    )
                    .unwrap(),
            );
            game.options.offscreen = true;
            game.renderer = Some(game.new_renderer(window.clone()).unwrap());
            game.title = None;
            game.player = Some(
                sunrise_client::motion::PlayerMotion::new(
                    game.motion,
                    sunrise_core::geometry::Vec3::ZERO,
                )
                .unwrap(),
            );
            assert_eq!(
                request(game, Some([0, 192])),
                Err(ScreenshotRequestError::InvalidSize)
            );
            assert!(!dir.exists());
            fs::write(&dir, "not a directory").unwrap();
            assert_eq!(
                request(game, None),
                Err(ScreenshotRequestError::OutputUnavailable)
            );
            fs::remove_file(&dir).unwrap();

            let elapsed = Duration::from_millis(16);
            game.extras_frame(elapsed);
            assert_eq!(fs::read_dir(&dir).unwrap().count(), 0, "queued, not saved");
            assert!(
                !game.render_and_flip(elapsed).unwrap(),
                "offscreen, not presented"
            );
            let renderer = game.renderer.as_ref().unwrap();
            assert_eq!([renderer.config.width, renderer.config.height], [320, 240]);
            let actual = window.inner_size();
            assert_eq!([actual.width, actual.height], [320, 240]);
            let projection = game.native_camera_frame.as_ref().unwrap().projection;
            assert_eq!(projection[0].to_bits(), (projection[5] * 0.5).to_bits());
            assert_eq!(request(game, None), Err(ScreenshotRequestError::Busy));

            let deadline = Instant::now() + Duration::from_secs(10);
            while game.ui.mod_screenshot.is_some() {
                game.poll_mod_screenshot();
                assert!(Instant::now() < deadline, "screenshot did not finish");
                std::thread::sleep(Duration::from_millis(5));
            }
            let entries = fs::read_dir(&dir)
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(entries.len(), 1);
            let png = fs::File::open(entries[0].path()).unwrap();
            let mut reader = png::Decoder::new(std::io::BufReader::new(png))
                .read_info()
                .unwrap();
            let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
            let image = reader.next_frame(&mut pixels).unwrap();
            assert_eq!([image.width, image.height], EXACT);
            assert_eq!(F2_EVENTS.load(Relaxed), 0);
            assert_eq!(
                request(game, None),
                Ok(()),
                "successful write released the reservation"
            );
        });
    });
}
