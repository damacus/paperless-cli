use std::borrow::Cow;

use ratatui::widgets::{Paragraph, Wrap};

const LINES_PER_CHUNK: usize = 32;
// A paragraph's local scroll offset is a u16. Cap the byte length of any
// individually wrapped chunk so even a long logical line fits that range.
const MAX_CHUNK_BYTES: usize = 32 * 1024 - 1;

#[derive(Clone, Debug, Default)]
pub(super) struct InspectorLayout {
    source: Option<(usize, usize, u16)>,
    chunks: Vec<Chunk>,
    end: usize,
    rows: usize,
}

#[derive(Clone, Debug)]
struct Chunk {
    start: usize,
    end: usize,
    first_row: usize,
}

impl InspectorLayout {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn clamp_scroll(
        &mut self,
        text: &str,
        width: u16,
        scroll: usize,
        height: usize,
    ) -> usize {
        if scroll == 0 || width == 0 {
            return 0;
        }
        self.index_to(text, width, scroll.saturating_add(height));
        scroll.min(self.rows.saturating_sub(height))
    }

    pub(super) fn visible<'a>(
        &mut self,
        text: &'a str,
        width: u16,
        scroll: usize,
        height: usize,
    ) -> (Cow<'a, str>, u16) {
        if width == 0 || height == 0 {
            return (Cow::Borrowed(""), 0);
        }
        if scroll == 0 {
            // Each logical line contributes at least one visual row. The first
            // frame can paint without indexing any of the remaining text.
            let end = chunk_end(text, 0, height);
            let slice = text.get(..end).unwrap_or_default();
            if end == text.len() || slice.lines().count() >= height {
                return (Cow::Borrowed(slice), 0);
            }
        }

        let bottom = scroll.saturating_add(height);
        self.index_to(text, width, bottom);
        let start = self
            .chunks
            .partition_point(|chunk| chunk.first_row <= scroll);
        let finish = self
            .chunks
            .partition_point(|chunk| chunk.first_row < bottom);
        let Some(chunks) = start
            .checked_sub(1)
            .and_then(|index| self.chunks.get(index..finish))
        else {
            return (Cow::Borrowed(""), 0);
        };
        let (Some(first), Some(last)) = (chunks.first(), chunks.last()) else {
            return (Cow::Borrowed(""), 0);
        };
        let offset = u16::try_from(scroll - first.first_row).unwrap_or_default();
        let slice = join_visible_text(text, chunks, first.start, last.end);
        (slice, offset)
    }

    fn index_to(&mut self, text: &str, width: u16, target: usize) {
        let source = (text.as_ptr().addr(), text.len(), width);
        if self.source != Some(source) {
            self.reset();
            self.source = Some(source);
        }

        while self.rows < target && self.end < text.len() {
            let end = chunk_end(text, self.end, LINES_PER_CHUNK);
            let Some(slice) = text.get(self.end..end) else {
                break;
            };
            // Use the same word wrapping as the render path, including Unicode
            // widths, whitespace and explicit line breaks.
            let rows = Paragraph::new(slice)
                .wrap(Wrap { trim: false })
                .line_count(width);
            self.chunks.push(Chunk {
                start: self.end,
                end,
                first_row: self.rows,
            });
            self.end = end;
            self.rows = self.rows.saturating_add(rows);
        }
    }
}

fn chunk_end(text: &str, start: usize, lines: usize) -> usize {
    let safe_end = text.floor_char_boundary(start.saturating_add(MAX_CHUNK_BYTES).min(text.len()));
    let prefix = text.get(start..safe_end).unwrap_or_default();
    if let Some((index, _)) = prefix.match_indices('\n').nth(lines - 1) {
        return start + index + 1;
    }
    if safe_end == text.len() {
        return safe_end;
    }
    // Keep the original newline whenever possible. Exceptionally long
    // logical lines get an extra wrap boundary at the clipped byte index.
    prefix
        .rfind('\n')
        .map_or(safe_end, |index| start + index + 1)
}

fn join_visible_text<'a>(
    text: &'a str,
    chunks: &[Chunk],
    start: usize,
    end: usize,
) -> Cow<'a, str> {
    let internal = chunks.iter().take(chunks.len().saturating_sub(1));
    if internal.clone().all(|chunk| has_newline(text, chunk.end)) {
        return Cow::Borrowed(text.get(start..end).unwrap_or_default());
    }

    let mut visible = String::with_capacity(end - start + chunks.len());
    let mut previous = start;
    for chunk in internal {
        if !has_newline(text, chunk.end) {
            visible.push_str(text.get(previous..chunk.end).unwrap_or_default());
            visible.push('\n');
            previous = chunk.end;
        }
    }
    visible.push_str(text.get(previous..end).unwrap_or_default());
    Cow::Owned(visible)
}

fn has_newline(text: &str, end: usize) -> bool {
    end.checked_sub(1)
        .and_then(|index| text.as_bytes().get(index))
        .is_some_and(|byte| *byte == b'\n')
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

    #[test]
    fn chunked_rows_match_the_full_paragraph_at_multiple_widths() {
        let text =
            "  parcel  東京\r\n\r\nAn invoice with wrapping and café headings\r\n".repeat(65);
        let mut layout = InspectorLayout::default();
        for width in [26, 9, 35] {
            let area = Rect::new(0, 0, width, 6);
            let full = Paragraph::new(text.as_str()).wrap(Wrap { trim: false });
            let bottom = full
                .line_count(width)
                .saturating_sub(usize::from(area.height));
            assert_eq!(layout.clamp_scroll(&text, width, usize::MAX, 6), bottom);
            for expected_scroll in [0, 5, 45, 113, bottom] {
                let (visible, offset) = layout.visible(&text, width, expected_scroll, 6);
                let mut actual = Buffer::empty(area);
                Paragraph::new(visible)
                    .wrap(Wrap { trim: false })
                    .scroll((offset, 0))
                    .render(area, &mut actual);
                let mut expected = Buffer::empty(area);
                full.clone()
                    .scroll((u16::try_from(expected_scroll).unwrap(), 0))
                    .render(area, &mut expected);
                assert_eq!(actual, expected);
            }
        }
    }

    #[test]
    fn global_scroll_can_reach_past_u16_rows() {
        let text = "row\n".repeat(70_000) + "last row";
        let mut layout = InspectorLayout::default();
        let bottom = layout.clamp_scroll(&text, 16, usize::MAX, 4);
        assert_eq!(bottom, 69_997);
        let (tail, offset) = layout.visible(&text, 16, bottom, 4);
        assert!(tail.contains("last row"));
        assert!(usize::from(offset) < LINES_PER_CHUNK);
    }

    #[test]
    fn inspector_can_scroll_to_the_end_of_a_single_oversized_line() {
        let text = "x".repeat(70_000) + " tail";
        let mut layout = InspectorLayout::default();
        let bottom = layout.clamp_scroll(&text, 1, usize::MAX, 4);
        assert!(bottom > usize::from(u16::MAX));
        let (tail, offset) = layout.visible(&text, 1, bottom, 4);
        let mut actual = Buffer::empty(Rect::new(0, 0, 1, 4));
        Paragraph::new(tail)
            .wrap(Wrap { trim: false })
            .scroll((offset, 0))
            .render(actual.area, &mut actual);
        let last_rows: String = actual
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        assert_eq!(last_rows, "tail");
    }
}
