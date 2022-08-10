#[derive(Debug)]
pub struct Event {
    /// The name of the event. This is unique for the skeleton.
    pub name: String,

    /// The integer value of the event. Assume 0 if omitted.
    pub int: i32, // TODO: Unsure.

    /// The float value of the event. Assume 0 if omitted.
    pub float: f32,

    /// The string value of the event. Assume null if omitted.
    pub string: Option<String>,

    /// The path to an audio file if this event is intended to play audio. Assume null if omitted.
    pub audio_path: Option<String>,

    /// The volume used to play the audio file. Assume 1 if omitted.
    pub audio_volume: f32,

    /// The stereo balance used to play the audio file. Assume 0 if omitted.
    pub audio_balance: f32,
}
