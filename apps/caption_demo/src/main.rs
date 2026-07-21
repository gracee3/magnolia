use caption_state::CaptionState;
use speech_to_text::SttEvent;
use std::io::{self, BufRead};

fn main() {
    println!("Caption reducer demo. Enter partial/final text lines; Ctrl-D exits.");
    let stdin = io::stdin();
    let mut state = CaptionState::default();
    let mut sequence = 0;
    for line in stdin.lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        sequence += 1;
        let (finality, text) = line
            .strip_prefix("final ")
            .map_or((false, line.as_str()), |text| (true, text));
        let event = if finality {
            SttEvent::Final {
                session_id: "demo".into(),
                segment_id: 1,
                text: text.into(),
                start_ms: 0,
                end_ms: sequence * 100,
                sequence,
            }
        } else {
            SttEvent::Partial {
                session_id: "demo".into(),
                segment_id: 1,
                text: text.into(),
                audio_end_ms: sequence * 100,
                sequence,
            }
        };
        state.apply(event);
        print!(
            "\r\x1b[2K{}{}",
            state.display_text(),
            if state.provisional.is_some() {
                " …"
            } else {
                ""
            }
        );
        if state.provisional.is_none() {
            println!();
        }
    }
}
