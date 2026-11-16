#[derive(Clone, Copy, Default)]
struct Cell {
    size: usize,  // bytes
    width: isize, // runes
    htab: bool,
}

const ESCAPE: u8 = 0xff;
const PADDING: isize = 2;

struct Writer {
    out: Vec<u8>,
    buf: Vec<u8>,
    pos: usize,
    cell: Cell,
    end_char: u8,
    lines: Vec<Vec<Cell>>,
    widths: Vec<isize>,
}

fn rune_count(b: &[u8]) -> isize {
    crate::gofs::text(b).chars().count() as isize
}

impl Writer {
    fn reset(&mut self) {
        self.buf.clear();
        self.pos = 0;
        self.cell = Cell::default();
        self.end_char = 0;
        self.lines = vec![Vec::new()];
        self.widths.clear();
    }
    fn update_width(&mut self) {
        self.cell.width += rune_count(&self.buf[self.pos..]);
        self.pos = self.buf.len();
    }
    fn append(&mut self, t: &[u8]) {
        self.buf.extend_from_slice(t);
        self.cell.size += t.len();
    }
    fn terminate_cell(&mut self, htab: bool) -> usize {
        self.cell.htab = htab;
        let line = self.lines.last_mut().unwrap();
        line.push(self.cell);
        self.cell = Cell::default();
        line.len()
    }
    fn end_escape(&mut self) {
        self.update_width();
        self.cell.width -= 2;
        self.end_char = 0;
    }
    fn write_padding(&mut self, textw: isize, cellw: isize) {
        self.out.extend(std::iter::repeat_n(b' ', (cellw - textw).max(0) as usize));
    }
    fn write_lines(&mut self, mut pos: usize, line0: usize, line1: usize) -> usize {
        for i in line0..line1 {
            let line = self.lines[i].clone();
            for (j, c) in line.iter().enumerate() {
                if c.size == 0 {
                    if j < self.widths.len() {
                        self.write_padding(c.width, self.widths[j]);
                    }
                } else {
                    let t = self.buf[pos..pos + c.size].to_vec();
                    self.out.extend_from_slice(&t);
                    pos += c.size;
                    if j < self.widths.len() {
                        self.write_padding(c.width, self.widths[j]);
                    }
                }
            }
            if i + 1 == self.lines.len() {
                let t = self.buf[pos..pos + self.cell.size].to_vec();
                self.out.extend_from_slice(&t);
                pos += self.cell.size;
            } else {
                self.out.push(b'\n');
            }
        }
        pos
    }
    fn format(&mut self, pos0: usize, mut line0: usize, line1: usize) -> usize {
        let mut pos = pos0;
        let column = self.widths.len();
        let mut this = line0;
        while this < line1 {
            if column + 1 >= self.lines[this].len() {
                this += 1;
                continue;
            }
            pos = self.write_lines(pos, line0, this);
            line0 = this;
            let mut width = 0;
            while this < line1 {
                let line = &self.lines[this];
                if column + 1 >= line.len() {
                    break;
                }
                width = width.max(line[column].width + PADDING);
                this += 1;
            }
            self.widths.push(width);
            pos = self.format(pos, line0, this);
            self.widths.pop();
            line0 = this;
        }
        self.write_lines(pos, line0, line1)
    }
    fn flush(&mut self) {
        if self.cell.size > 0 {
            if self.end_char != 0 {
                self.end_escape();
            }
            self.terminate_cell(false);
        }
        let n = self.lines.len();
        self.format(0, 0, n);
        self.reset();
    }
    fn write(&mut self, buf: &[u8]) {
        let mut n = 0;
        for (i, &ch) in buf.iter().enumerate() {
            if self.end_char == 0 {
                match ch {
                    b'\t' | b'\x0b' | b'\n' | b'\x0c' => {
                        self.append(&buf[n..i]);
                        self.update_width();
                        n = i + 1;
                        let ncells = self.terminate_cell(ch == b'\t');
                        if ch == b'\n' || ch == b'\x0c' {
                            self.lines.push(Vec::new());
                            if ch == b'\x0c' || ncells == 1 {
                                self.flush_lines();
                            }
                        }
                    }
                    ESCAPE => {
                        self.append(&buf[n..i]);
                        self.update_width();
                        n = i;
                        self.end_char = ESCAPE;
                    }
                    _ => {}
                }
            } else if ch == self.end_char {
                self.append(&buf[n..i + 1]);
                n = i + 1;
                self.end_escape();
            }
        }
        self.append(&buf[n..]);
    }
    fn flush_lines(&mut self) {
        self.flush();
    }
}

pub fn format(text: &[u8]) -> Vec<u8> {
    let mut w = Writer {
        out: Vec::new(),
        buf: Vec::new(),
        pos: 0,
        cell: Cell::default(),
        end_char: 0,
        lines: vec![Vec::new()],
        widths: Vec::new(),
    };
    w.write(text);
    w.flush();
    w.out
}
