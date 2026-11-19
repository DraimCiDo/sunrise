use super::*;
use mod_api::prelude::*;
use mod_api::{
    AudioRefusal, AudioStreamRef, AudioStreamSource, AudioStreamState, SoundSource, SpecFault,
    StreamPlacement, StreamSpec,
};
use std::sync::Mutex;

/// What the game's stream was asked, as typed calls.
#[derive(Clone, Debug, PartialEq)]
enum Call {
    Push(Vec<i16>),
    Placement(StreamPlacement),
    Volume(f32),
    Stop,
}

/// The game's side of one open stream: it records the calls the guest makes, and, as the
/// engine's handle does, stops when it is let go.
struct Stream(Arc<Mutex<Vec<Call>>>);
impl Stream {
    fn record(&self, call: Call) {
        self.0.lock().unwrap().push(call);
    }
}
impl AudioStreamSource for Stream {
    fn push(&self, samples: &[i16]) -> u32 {
        self.record(Call::Push(samples.to_vec()));
        samples.len() as u32 / 2
    }
    fn set_placement(&self, placement: &StreamPlacement) -> bool {
        self.record(Call::Placement(*placement));
        !matches!(placement, StreamPlacement::At(_))
    }
    fn set_volume(&self, volume: f32) -> bool {
        self.record(Call::Volume(volume));
        true
    }
    fn state(&self) -> AudioStreamState {
        AudioStreamState::Unplaced
    }
    fn buffered_ms(&self) -> u32 {
        42
    }
    fn stop(&self) {
        self.record(Call::Stop);
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A client that opens a stream for mono and stereo specs and refuses any other channel count,
/// keeping what it was asked.
#[derive(Default)]
struct Client {
    opened: Mutex<Vec<StreamSpec>>,
    calls: Arc<Mutex<Vec<Call>>>,
}
impl mod_api::Host for Client {}
mod_api::impl_host_areas!(Client);
impl mod_api::ClientHost for Client {}
mod_api::impl_client_host_areas!(Client, except(audio));
impl AudioClientHost for Client {
    fn open_audio_stream(&self, spec: &StreamSpec) -> Result<AudioStreamRef, AudioRefusal> {
        self.opened.lock().unwrap().push(spec.clone());
        match spec.channels {
            1 | 2 => Ok(AudioStreamRef::new(Arc::new(Stream(self.calls.clone())))),
            _ => Err(AudioRefusal::BadSpec(SpecFault::Channels)),
        }
    }
}

#[test]
fn wire_loopback_a_guest_holds_an_audio_stream_by_id_and_drops_it_to_stop_it() {
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
    let client = Client::default();
    let mut frame = boundary::Frame::default();
    frame.set_client_host(&client);
    let mut cx = boundary::Cx::default();
    cx.frames.push(frame);
    let cx = Box::into_raw(Box::new(cx));
    let _guest = guest::resource_scope();
    // SAFETY: the context outlives every call; the transport is cleared before it is freed.
    guest::set_transport(Some(unsafe { host_dispatch::transport(cx) }));

    let spec = StreamSpec::new(SoundSource::Voice, 1, 24_000)
        .with_placement(StreamPlacement::At([1.5, 64.0, -3.0]))
        .with_prebuffer_ms(150);
    let stream = guest::Guest.open_audio_stream(&spec).unwrap();
    assert_eq!(*client.opened.lock().unwrap(), [spec]);
    assert_eq!(stream.push(&[1, -2, 3, 4]), 2);
    assert!(!stream.set_placement(&StreamPlacement::At([0.0, 1.0, 2.0])));
    assert!(stream.set_placement(&StreamPlacement::Flat));
    assert!(stream.set_volume(0.5));
    assert_eq!(stream.state(), AudioStreamState::Unplaced);
    assert_eq!(stream.buffered_ms(), 42);
    // `stop` crosses as a call of its own.
    stream.stop();
    // A refusal crosses by name.
    let refused = guest::Guest.open_audio_stream(&StreamSpec::new(SoundSource::Voice, 3, 24_000));
    assert_eq!(
        refused.err(),
        Some(AudioRefusal::BadSpec(SpecFault::Channels))
    );
    // Dropping the handle lets the game's stream go, which stops it.
    drop(stream);
    guest::set_transport(None);

    assert_eq!(
        *client.calls.lock().unwrap(),
        [
            Call::Push(vec![1, -2, 3, 4]),
            Call::Placement(StreamPlacement::At([0.0, 1.0, 2.0])),
            Call::Placement(StreamPlacement::Flat),
            Call::Volume(0.5),
            Call::Stop,
            Call::Stop,
        ]
    );
    // SAFETY: the transport that held the pointer is gone.
    drop(unsafe { Box::from_raw(cx) });
}

#[test]
fn wire_loopback_an_open_in_the_wrong_phase_is_refused_by_name_without_reaching_the_host() {
    // Packages allows no host call; the client is never reached, so none is bound.
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Packages);
    let cx = Box::into_raw(Box::new(boundary::Cx::default()));
    let _guest = guest::resource_scope();
    // SAFETY: the context outlives every call; the transport is cleared before it is freed.
    guest::set_transport(Some(unsafe { host_dispatch::transport(cx) }));
    let result = guest::Guest.open_audio_stream(&StreamSpec::new(SoundSource::Voice, 1, 48_000));
    guest::set_transport(None);
    assert_eq!(result.err(), Some(AudioRefusal::Unavailable));
    // SAFETY: the transport that held the pointer is gone.
    drop(unsafe { Box::from_raw(cx) });
}
