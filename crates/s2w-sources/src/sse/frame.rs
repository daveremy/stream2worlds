//! SSE framing: bytes to `id:`/`data:` frames, per the WHATWG event-stream rules we rely on.

#[derive(Default)]
pub(super) struct FrameParser {
    buffered: Vec<u8>,
    frame: FrameBuilder,
}

impl FrameParser {
    pub(super) fn push(&mut self, chunk: &[u8]) -> Vec<Result<RawFrame, String>> {
        self.buffered.extend_from_slice(chunk);
        let mut consumed = 0;
        let mut output = Vec::new();
        while let Some((line_end, delimiter_len)) = complete_line(&self.buffered, consumed) {
            let line = self.buffered[consumed..line_end].to_vec();
            consumed = line_end + delimiter_len;
            match String::from_utf8(line) {
                Ok(line) => {
                    if line.is_empty() {
                        if let Some(frame) = self.frame.take() {
                            output.push(Ok(frame));
                        }
                    } else {
                        self.frame.push_line(&line);
                    }
                }
                Err(error) => {
                    self.frame = FrameBuilder::default();
                    output.push(Err(format!("SSE line is not valid UTF-8: {error}")));
                }
            }
        }
        if consumed > 0 {
            self.buffered.drain(..consumed);
        }
        output
    }

    pub(super) fn finish(&mut self) -> Vec<Result<RawFrame, String>> {
        if self.buffered.last() == Some(&b'\r') {
            self.push(b"\n")
        } else {
            Vec::new()
        }
    }
}

fn complete_line(buffer: &[u8], start: usize) -> Option<(usize, usize)> {
    for index in start..buffer.len() {
        match buffer[index] {
            b'\n' => return Some((index, 1)),
            b'\r' if index + 1 == buffer.len() => return None,
            b'\r' if buffer[index + 1] == b'\n' => return Some((index, 2)),
            b'\r' => return Some((index, 1)),
            _ => {}
        }
    }
    None
}

#[derive(Default)]
struct FrameBuilder {
    id: Option<String>,
    data: Vec<String>,
}

impl FrameBuilder {
    fn push_line(&mut self, line: &str) {
        if line.starts_with(':') {
            return;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "id" if !value.contains('\0') => self.id = Some(value.to_owned()),
            "data" => self.data.push(value.to_owned()),
            _ => {}
        }
    }

    fn take(&mut self) -> Option<RawFrame> {
        if self.id.is_none() && self.data.is_empty() {
            return None;
        }
        let id = self.id.take();
        let data = if self.data.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.data).join("\n"))
        };
        Some(RawFrame { id, data })
    }
}

pub(super) struct RawFrame {
    pub(super) id: Option<String>,
    pub(super) data: Option<String>,
}

/// The frames of a recorded event stream that carry both an `id:` and a `data:` field, as
/// `(id, data)` in stream order, cut by the same framing a live SSE source uses. For replaying a
/// recording (the scale fixtures in `s2w-app`, xtask's check 12), so every reader of a recording
/// shares this one parser. A live source also runs its dialect over each frame; this does not.
/// A final frame with no terminating blank line is dropped, as a live source never delivers it.
///
/// # Errors
///
/// A line that is not valid UTF-8, with the parser's reason.
pub fn replay_frames(bytes: &[u8]) -> Result<Vec<(String, String)>, String> {
    let mut parser = FrameParser::default();
    let mut frames = parser.push(bytes);
    frames.extend(parser.finish());
    let mut out = Vec::with_capacity(frames.len());
    for frame in frames {
        if let RawFrame {
            id: Some(id),
            data: Some(data),
        } = frame?
        {
            out.push((id, data));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::replay_frames;

    #[test]
    fn replay_keeps_frames_with_an_id_and_data_and_joins_multi_line_data() {
        let text = ": comment\nevent: m\nid: 1\ndata: a\ndata: b\n\ndata: orphan\n\nid: 2\n\nid: 3\r\ndata: c\r\n\r\nid: 4\ndata: unterminated\n";
        assert_eq!(
            replay_frames(text.as_bytes()),
            Ok(vec![
                ("1".to_owned(), "a\nb".to_owned()),
                ("3".to_owned(), "c".to_owned()),
            ])
        );
    }

    #[test]
    fn replay_refuses_a_line_that_is_not_utf8() {
        assert!(replay_frames(b"id: 1\ndata: \xff\n\n").is_err());
    }
}
