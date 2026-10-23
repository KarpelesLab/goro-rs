//! Colour reduction (QuantizeImage(), QuantizeImages(), RemapImage(),
//! dithered PosterizeImage()) with ImageMagick's colour cube.
//!
//! The algorithm is the one ImageMagick documents for `-colors`
//! (classification, reduction, assignment over an octree of the RGB
//! cube); the details below were pinned down by black-box comparison
//! with `magick` 7.1.2 Q16:
//!
//! - a pixel's path through the tree follows the bits of its 8-bit
//!   channel values (red is child bit 0, green bit 1, blue bit 2);
//!   node centres halve the Q16 range from QuantumRange/2;
//! - a node's error is the sum, over the pixels that pass through it,
//!   of their Euclidean distance (in QuantumScale units) to its centre;
//! - reduction repeats passes that prune every node whose error is at
//!   most the threshold (children first), the next threshold being the
//!   smallest error left, until no more than the wanted colours remain;
//! - the default tree depth is 1 + the number of base-4 digits of the
//!   colour count, one less when dithering, at most 8;
//! - a colour is the rounded mean of the pixels merged into its node;
//! - a pixel takes the closest colour (Euclidean RGB, later colours of a
//!   depth-first walk winning ties) under the parent of the deepest node
//!   on its path;
//! - Floyd-Steinberg runs serpentine (7/16, 3/16, 5/16, 1/16), the
//!   Riemersma dither follows a Hilbert curve and spreads the error of
//!   the last 16 pixels with weights from 1/256 (newest) to 16/256
//!   (oldest); both round the error-adjusted pixel to a quantum and
//!   remember the colour chosen for each 6-bit-per-channel cell.

use crate::image::Image;

const NONE: u32 = u32::MAX;
const Q: f64 = 65535.0;

/// DitherMethod (no dither, Riemersma, Floyd-Steinberg).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dither {
    None,
    Riemersma,
    FloydSteinberg,
}

#[derive(Clone)]
struct Node {
    child: [u32; 8],
    parent: u32,
    id: u8,
    /// pixels merged into this node (ImageMagick's n2)
    count: f64,
    sum: [f64; 3],
    error: f64,
    mid: [f64; 3],
    color: [u16; 3],
}

impl Node {
    fn new(parent: u32, id: u8, mid: [f64; 3]) -> Node {
        Node {
            child: [NONE; 8],
            parent,
            id,
            count: 0.0,
            sum: [0.0; 3],
            error: 0.0,
            mid,
            color: [0; 3],
        }
    }
}

fn char8(v: u16) -> u32 {
    ((v as f64) / 257.0).round() as u32
}

fn child_id(c: &[u32; 3], level: usize) -> usize {
    let sh = 7 - level;
    (((c[0] >> sh) & 1) | (((c[1] >> sh) & 1) << 1) | (((c[2] >> sh) & 1) << 2)) as usize
}

/// The colour cube.
pub struct Cube {
    nodes: Vec<Node>,
    depth: usize,
}

impl Cube {
    pub fn new(depth: usize) -> Cube {
        Cube {
            nodes: vec![Node::new(NONE, 0, [Q / 2.0; 3])],
            depth: depth.clamp(1, 8),
        }
    }

    /// The default depth for `colors` colours.
    pub fn default_depth(colors: usize, dither: bool) -> usize {
        let mut depth = 1usize;
        let mut c = colors;
        while c != 0 {
            c >>= 2;
            depth += 1;
        }
        if dither {
            depth -= 1;
        }
        depth.clamp(1, 8)
    }

    /// Classification: `count` pixels of colour `c`.
    pub fn add(&mut self, c: [u16; 3], count: f64) {
        let b = [char8(c[0]), char8(c[1]), char8(c[2])];
        let cf = [c[0] as f64, c[1] as f64, c[2] as f64];
        let mut bisect = Q / 2.0;
        let mut node = 0usize;
        for level in 0..self.depth {
            bisect /= 2.0;
            let id = child_id(&b, level);
            let mut next = self.nodes[node].child[id];
            if next == NONE {
                let mut mid = self.nodes[node].mid;
                for (k, m) in mid.iter_mut().enumerate() {
                    *m += if (id >> k) & 1 == 1 { bisect } else { -bisect };
                }
                self.nodes.push(Node::new(node as u32, id as u8, mid));
                next = (self.nodes.len() - 1) as u32;
                self.nodes[node].child[id] = next;
            }
            node = next as usize;
            let n = &mut self.nodes[node];
            let d: f64 = (0..3)
                .map(|k| {
                    let t = (cf[k] - n.mid[k]) / Q;
                    t * t
                })
                .sum();
            n.error += count * d.sqrt();
        }
        let n = &mut self.nodes[node];
        n.count += count;
        for k in 0..3 {
            n.sum[k] += count * cf[k];
        }
    }

    fn add_image(&mut self, img: &Image) {
        // runs of equal pixels are classified together
        let mut px = img.pixels.chunks_exact(4).map(|p| [p[0], p[1], p[2]]);
        let Some(mut cur) = px.next() else {
            return;
        };
        let mut run = 1.0;
        for p in px {
            if p == cur {
                run += 1.0;
            } else {
                self.add(cur, run);
                cur = p;
                run = 1.0;
            }
        }
        self.add(cur, run);
    }

    fn colors(&self, n: usize) -> usize {
        let nd = &self.nodes[n];
        let mut c = usize::from(nd.count > 0.0);
        for &ch in &nd.child {
            if ch != NONE {
                c += self.colors(ch as usize);
            }
        }
        c
    }

    fn prune(&mut self, n: usize) {
        for i in 0..8 {
            let c = self.nodes[n].child[i];
            if c != NONE {
                self.prune(c as usize);
            }
        }
        let (count, sum, id, p) = {
            let nd = &self.nodes[n];
            (nd.count, nd.sum, nd.id, nd.parent as usize)
        };
        let pn = &mut self.nodes[p];
        pn.count += count;
        for k in 0..3 {
            pn.sum[k] += sum[k];
        }
        pn.child[id as usize] = NONE;
    }

    fn reduce_pass(&mut self, n: usize, threshold: f64, next: &mut f64, colors: &mut usize) {
        for i in 0..8 {
            let c = self.nodes[n].child[i];
            if c != NONE {
                self.reduce_pass(c as usize, threshold, next, colors);
            }
        }
        if n != 0 && self.nodes[n].error <= threshold {
            self.prune(n);
        } else {
            if self.nodes[n].count > 0.0 {
                *colors += 1;
            }
            if n != 0 && self.nodes[n].error < *next {
                *next = self.nodes[n].error;
            }
        }
    }

    /// Reduction to at most `max` colours, then the colour map.
    pub fn reduce(&mut self, max: usize) {
        let mut colors = self.colors(0);
        let mut next = 0.0;
        while colors > max.max(1) {
            let threshold = next;
            next = f64::MAX;
            colors = 0;
            self.reduce_pass(0, threshold, &mut next, &mut colors);
        }
        for nd in self.nodes.iter_mut() {
            if nd.count > 0.0 {
                for k in 0..3 {
                    nd.color[k] = (nd.sum[k] / nd.count + 0.5).floor().clamp(0.0, Q) as u16;
                }
            }
        }
    }

    fn closest(&self, n: usize, c: &[f64; 3], best: &mut (f64, [u16; 3])) {
        let nd = &self.nodes[n];
        if nd.count > 0.0 {
            let d: f64 = (0..3)
                .map(|k| {
                    let t = c[k] - nd.color[k] as f64;
                    t * t
                })
                .sum();
            if d <= best.0 {
                *best = (d, nd.color);
            }
        }
        for &ch in &nd.child {
            if ch != NONE {
                self.closest(ch as usize, c, best);
            }
        }
    }

    /// Assignment: the colour a (quantum-rounded) colour maps to.
    pub fn lookup(&self, c: [u16; 3]) -> [u16; 3] {
        let b = [char8(c[0]), char8(c[1]), char8(c[2])];
        let mut node = 0usize;
        for level in 0..self.depth {
            let next = self.nodes[node].child[child_id(&b, level)];
            if next == NONE {
                break;
            }
            node = next as usize;
        }
        if node != 0 {
            node = self.nodes[node].parent as usize;
        }
        let mut best = (f64::MAX, [0u16; 3]);
        let cf = [c[0] as f64, c[1] as f64, c[2] as f64];
        self.closest(node, &cf, &mut best);
        best.1
    }

    /// The colour map, in tree order.
    pub fn palette(&self) -> Vec<[u16; 3]> {
        let mut out = vec![];
        self.walk(0, &mut out);
        out
    }

    fn walk(&self, n: usize, out: &mut Vec<[u16; 3]>) {
        let nd = &self.nodes[n];
        if nd.count > 0.0 {
            out.push(nd.color);
        }
        for &ch in &nd.child {
            if ch != NONE {
                self.walk(ch as usize, out);
            }
        }
    }

    /// Maps every pixel of `img` (alpha untouched) with `dither`.
    pub fn assign(&self, img: &mut Image, dither: Dither) {
        match dither {
            Dither::None => {
                let mut cache = std::collections::HashMap::new();
                for p in img.pixels.chunks_exact_mut(4) {
                    let k = [p[0], p[1], p[2]];
                    let c = *cache.entry(k).or_insert_with(|| self.lookup(k));
                    p[..3].copy_from_slice(&c);
                }
            }
            Dither::FloydSteinberg => self.floyd_steinberg(img),
            Dither::Riemersma => self.riemersma(img),
        }
        img.colors = None;
    }

    fn floyd_steinberg(&self, img: &mut Image) {
        let (w, h) = (img.columns, img.rows);
        let mut cache = DitherCache::new();
        let mut cur = vec![[0f64; 3]; w + 2];
        let mut next = vec![[0f64; 3]; w + 2];
        for y in 0..h {
            let ltr = y % 2 == 0;
            for i in 0..w {
                let x = if ltr { i } else { w - 1 - i };
                let at = (y * w + x) * 4;
                let e = cur[x + 1];
                let mut v = [0u16; 3];
                for k in 0..3 {
                    v[k] = (img.pixels[at + k] as f64 + e[k]).clamp(0.0, Q).round() as u16;
                }
                let c = cache.get(self, v);
                let (fwd, back) = if ltr { (x + 2, x) } else { (x, x + 2) };
                for k in 0..3 {
                    let err = v[k] as f64 - c[k] as f64;
                    cur[fwd][k] += err * 7.0 / 16.0;
                    next[back][k] += err * 3.0 / 16.0;
                    next[x + 1][k] += err * 5.0 / 16.0;
                    next[fwd][k] += err / 16.0;
                }
                img.pixels[at..at + 3].copy_from_slice(&c);
            }
            std::mem::swap(&mut cur, &mut next);
            next.iter_mut().for_each(|e| *e = [0.0; 3]);
        }
    }

    fn riemersma(&self, img: &mut Image) {
        let (w, h) = (img.columns, img.rows);
        let mut weights = [0f64; 16];
        for (j, wt) in weights.iter_mut().enumerate() {
            // errors[0] is the oldest
            *wt = 16f64.powf((15 - j) as f64 / 15.0) / 256.0;
        }
        let mut r = Hilbert {
            cube: self,
            img,
            w: w as i64,
            h: h as i64,
            x: 0,
            y: 0,
            errors: [[0.0; 3]; 16],
            weights,
            cache: DitherCache::new(),
        };
        let mut level = 0;
        while (1usize << level) < w.max(h) {
            level += 1;
        }
        if level > 0 {
            r.curve(level, Dir::Up);
        }
        r.step(Dir::None);
    }
}

/// The colours chosen for each 6-bit-per-channel cell while dithering.
struct DitherCache {
    slot: Vec<u32>,
    colors: Vec<[u16; 3]>,
}

impl DitherCache {
    fn new() -> DitherCache {
        DitherCache {
            slot: vec![NONE; 1 << 18],
            colors: vec![],
        }
    }
    fn get(&mut self, cube: &Cube, v: [u16; 3]) -> [u16; 3] {
        let key =
            ((char8(v[0]) >> 2) | ((char8(v[1]) >> 2) << 6) | ((char8(v[2]) >> 2) << 12)) as usize;
        let e = self.slot[key];
        if e != NONE {
            return self.colors[e as usize];
        }
        let c = cube.lookup(v);
        self.slot[key] = self.colors.len() as u32;
        self.colors.push(c);
        c
    }
}

#[derive(Clone, Copy)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
    None,
}

/// Riemersma's dither along a Hilbert curve over the enclosing
/// power-of-two square (points outside the image are skipped).
struct Hilbert<'a> {
    cube: &'a Cube,
    img: &'a mut Image,
    w: i64,
    h: i64,
    x: i64,
    y: i64,
    errors: [[f64; 3]; 16],
    weights: [f64; 16],
    cache: DitherCache,
}

impl Hilbert<'_> {
    fn step(&mut self, d: Dir) {
        if self.x >= 0 && self.y >= 0 && self.x < self.w && self.y < self.h {
            let at = ((self.y * self.w + self.x) * 4) as usize;
            let mut v = [0u16; 3];
            for k in 0..3 {
                let e: f64 = (0..16).map(|j| self.errors[j][k] * self.weights[j]).sum();
                v[k] = (self.img.pixels[at + k] as f64 + e).clamp(0.0, Q).round() as u16;
            }
            let c = self.cache.get(self.cube, v);
            self.errors.copy_within(1.., 0);
            self.errors[15] = [
                v[0] as f64 - c[0] as f64,
                v[1] as f64 - c[1] as f64,
                v[2] as f64 - c[2] as f64,
            ];
            self.img.pixels[at..at + 3].copy_from_slice(&c);
        }
        match d {
            Dir::Up => self.y -= 1,
            Dir::Down => self.y += 1,
            Dir::Left => self.x -= 1,
            Dir::Right => self.x += 1,
            Dir::None => {}
        }
    }

    fn curve(&mut self, level: u32, d: Dir) {
        use Dir::*;
        if level == 1 {
            let s = match d {
                Left => [Right, Down, Left],
                Right => [Left, Up, Right],
                Up => [Down, Right, Up],
                _ => [Up, Left, Down],
            };
            for m in s {
                self.step(m);
            }
            return;
        }
        let (a, b, c, e, m1, m2, m3) = match d {
            Left => (Up, Left, Left, Down, Right, Down, Left),
            Right => (Down, Right, Right, Up, Left, Up, Right),
            Up => (Left, Up, Up, Right, Down, Right, Up),
            _ => (Right, Down, Down, Left, Up, Left, Down),
        };
        self.curve(level - 1, a);
        self.step(m1);
        self.curve(level - 1, b);
        self.step(m2);
        self.curve(level - 1, c);
        self.step(m3);
        self.curve(level - 1, e);
    }
}

/// Rec. 709 luma, rounded to a quantum.
fn luma(p: &[u16]) -> u16 {
    (0.212656 * p[0] as f64 + 0.715158 * p[1] as f64 + 0.072186 * p[2] as f64)
        .round()
        .clamp(0.0, Q) as u16
}

fn unique_colors(imgs: &[&Image], limit: usize) -> Option<usize> {
    let mut seen = std::collections::HashSet::new();
    for i in imgs {
        for p in i.pixels.chunks_exact(4) {
            seen.insert([p[0], p[1], p[2]]);
            if seen.len() > limit {
                return None;
            }
        }
    }
    Some(seen.len())
}

/// QuantizeImages(): one colour map for every image. `tree_depth` 0
/// picks the default depth. Images with no more colours than asked
/// for are left as they are.
pub fn quantize_images(imgs: &mut [&mut Image], colors: usize, tree_depth: usize, dither: Dither) {
    let colors = colors.max(1);
    {
        let all: Vec<&Image> = imgs.iter().map(|i| &**i).collect();
        if let Some(n) = unique_colors(&all, colors) {
            for i in imgs.iter_mut() {
                i.colors = Some(n);
            }
            return;
        }
    }
    let depth = if tree_depth == 0 {
        Cube::default_depth(colors, dither != Dither::None)
    } else {
        tree_depth
    };
    let mut cube = Cube::new(depth);
    for i in imgs.iter() {
        cube.add_image(i);
    }
    cube.reduce(colors);
    let n = cube.palette().len();
    for i in imgs.iter_mut() {
        cube.assign(i, dither);
        i.colors = Some(n);
    }
}

/// QuantizeImage().
pub fn quantize(img: &mut Image, colors: usize, tree_depth: usize, dither: Dither) {
    quantize_images(&mut [img], colors, tree_depth, dither);
}

/// RemapImage(): the closest colours of `reference`'s, dithered. A gray
/// reference maps the pixels' luma.
pub fn remap(img: &mut Image, reference: &Image, dither: Dither) {
    let mut cube = Cube::new(8);
    let mut seen = std::collections::HashSet::new();
    let mut gray = true;
    for p in reference.pixels.chunks_exact(4) {
        let c = [p[0], p[1], p[2]];
        if seen.insert(c) {
            gray &= c[0] == c[1] && c[1] == c[2];
            cube.add(c, 1.0);
        }
    }
    if seen.is_empty() {
        cube.add([0, 0, 0], 1.0);
    }
    cube.reduce(usize::MAX);
    if gray {
        for p in img.pixels.chunks_exact_mut(4) {
            let l = luma(p);
            p[..3].fill(l);
        }
    }
    let n = cube.palette().len();
    cube.assign(img, dither);
    img.colors = Some(n);
}

/// Maps `img` onto an explicit colour map (PosterizeImage()'s levels).
pub fn map_to_palette(img: &mut Image, palette: &[[u16; 3]], dither: Dither) {
    let mut cube = Cube::new(8);
    for c in palette {
        cube.add(*c, 1.0);
    }
    cube.reduce(usize::MAX);
    cube.assign(img, dither);
}
