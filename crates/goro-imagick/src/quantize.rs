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
    child: [u32; 16],
    parent: u32,
    id: u8,
    /// pixels merged into this node (ImageMagick's n2)
    count: f64,
    sum: [f64; 4],
    error: f64,
    mid: [f64; 4],
    color: [u16; 4],
    /// `color` as compared (alpha-weighted in an RGBA cube)
    assoc: [f64; 4],
}

impl Node {
    fn new(parent: u32, id: u8, mid: [f64; 4]) -> Node {
        Node {
            child: [NONE; 16],
            parent,
            id,
            count: 0.0,
            sum: [0.0; 4],
            error: 0.0,
            mid,
            color: [0; 4],
            assoc: [0.0; 4],
        }
    }
}

fn char8(v: u16) -> u32 {
    ((v as f64) / 257.0).round() as u32
}

fn child_id(c: &[u32; 4], level: usize, nch: usize) -> usize {
    let sh = 7 - level;
    (0..nch).map(|k| ((c[k] >> sh) & 1) << k).sum::<u32>() as usize
}

/// An RGBA cube works on alpha-weighted (associated) colour.
const ASSOC: bool = true;

fn assoc(c: [u16; 4], nch: usize) -> [f64; 4] {
    let mut f = c.map(|v| v as f64);
    if nch == 4 && ASSOC {
        let a = f[3] / Q;
        for v in f.iter_mut().take(3) {
            *v *= a;
        }
    }
    f
}

/// A pixel's colour, alpha opaque when the image has none.
pub(crate) fn px4(p: &[u16], alpha: bool) -> [u16; 4] {
    [p[0], p[1], p[2], if alpha { p[3] } else { 65535 }]
}

/// The colour cube.
pub struct Cube {
    nodes: Vec<Node>,
    depth: usize,
    /// 3 (RGB) or 4 (RGBA) channels
    nch: usize,
}

impl Cube {
    pub fn new(depth: usize, alpha: bool) -> Cube {
        Cube {
            nodes: vec![Node::new(NONE, 0, [Q / 2.0; 4])],
            depth: depth.clamp(1, 8),
            nch: if alpha { 4 } else { 3 },
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
    pub fn add(&mut self, c: [u16; 4], count: f64) {
        let nch = self.nch;
        let cf = self.assoc(c);
        let b = cf.map(|v| (v / 257.0).round() as u32);
        let mut bisect = Q / 2.0;
        let mut node = 0usize;
        for level in 0..self.depth {
            bisect /= 2.0;
            let id = child_id(&b, level, nch);
            let mut next = self.nodes[node].child[id];
            if next == NONE {
                let mut mid = self.nodes[node].mid;
                for (k, m) in mid.iter_mut().enumerate().take(nch) {
                    *m += if (id >> k) & 1 == 1 { bisect } else { -bisect };
                }
                self.nodes.push(Node::new(node as u32, id as u8, mid));
                next = (self.nodes.len() - 1) as u32;
                self.nodes[node].child[id] = next;
            }
            node = next as usize;
            let n = &mut self.nodes[node];
            let d: f64 = (0..nch)
                .map(|k| {
                    let t = (cf[k] - n.mid[k]) / Q;
                    t * t
                })
                .sum();
            n.error += count * d.sqrt();
        }
        let n = &mut self.nodes[node];
        n.count += count;
        for k in 0..4 {
            n.sum[k] += count * cf[k];
        }
    }

    fn add_image(&mut self, img: &Image) {
        // runs of equal pixels are classified together
        let alpha = self.nch == 4;
        let mut px = img.pixels.chunks_exact(4).map(|p| px4(p, alpha));
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
        for i in 0..16 {
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
        for k in 0..4 {
            pn.sum[k] += sum[k];
        }
        pn.child[id as usize] = NONE;
    }

    fn reduce_pass(&mut self, n: usize, threshold: f64, next: &mut f64, colors: &mut usize) {
        for i in 0..16 {
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
        let nch = self.nch;
        for nd in self.nodes.iter_mut() {
            if nd.count > 0.0 {
                let mut m = [0.0; 4];
                for k in 0..4 {
                    m[k] = nd.sum[k] / nd.count;
                }
                if nch == 4 && ASSOC {
                    // means of alpha-weighted colour, back to unassociated
                    let a = m[3] / Q;
                    if a > 0.0 {
                        for v in m.iter_mut().take(3) {
                            *v /= a;
                        }
                    }
                }
                for k in 0..4 {
                    nd.color[k] = (m[k] + 0.5).floor().clamp(0.0, Q) as u16;
                }
                nd.assoc = assoc(nd.color, nch);
            }
        }
    }

    fn closest(&self, n: usize, c: &[f64; 4], best: &mut (f64, [u16; 4])) {
        let nd = &self.nodes[n];
        if nd.count > 0.0 {
            let d: f64 = (0..self.nch)
                .map(|k| {
                    let t = c[k] - nd.assoc[k];
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
    pub fn lookup(&self, c: [u16; 4]) -> [u16; 4] {
        let cf = self.assoc(c);
        let b = cf.map(|v| (v / 257.0).round() as u32);
        let mut node = 0usize;
        for level in 0..self.depth {
            let next = self.nodes[node].child[child_id(&b, level, self.nch)];
            if next == NONE {
                break;
            }
            node = next as usize;
        }
        if node != 0 {
            node = self.nodes[node].parent as usize;
        }
        let mut best = (f64::MAX, [0u16; 4]);
        self.closest(node, &cf, &mut best);
        best.1
    }

    fn assoc(&self, c: [u16; 4]) -> [f64; 4] {
        assoc(c, self.nch)
    }

    /// The colour map, in tree order.
    pub fn palette(&self) -> Vec<[u16; 4]> {
        let mut out = vec![];
        self.walk(0, &mut out);
        out
    }

    fn walk(&self, n: usize, out: &mut Vec<[u16; 4]>) {
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

    /// Maps every pixel of `img` with `dither`.
    pub fn assign(&self, img: &mut Image, dither: Dither) {
        self.assign_with(img, dither, &mut DitherCache::new(self.nch));
    }

    /// assign(), with the dither's colour cache carried over from
    /// earlier images.
    fn assign_with(&self, img: &mut Image, dither: Dither, cache: &mut DitherCache) {
        let alpha = self.nch == 4;
        match dither {
            Dither::None => {
                let mut cache = std::collections::HashMap::new();
                for p in img.pixels.chunks_exact_mut(4) {
                    let k = px4(p, alpha);
                    let c = *cache.entry(k).or_insert_with(|| self.lookup(k));
                    p[..self.nch].copy_from_slice(&c[..self.nch]);
                }
            }
            Dither::FloydSteinberg => self.floyd_steinberg(img, cache),
            Dither::Riemersma => self.riemersma(img, cache),
        }
        img.colors = None;
    }

    fn floyd_steinberg(&self, img: &mut Image, cache: &mut DitherCache) {
        let (w, h, nch, alpha) = (img.columns, img.rows, self.nch, self.nch == 4);
        let mut cur = vec![[0f64; 4]; w + 2];
        let mut next = vec![[0f64; 4]; w + 2];
        for y in 0..h {
            let ltr = y % 2 == 0;
            for i in 0..w {
                let x = if ltr { i } else { w - 1 - i };
                let at = (y * w + x) * 4;
                let e = cur[x + 1];
                let mut v = px4(&img.pixels[at..at + 4], alpha);
                for k in 0..nch {
                    v[k] = (v[k] as f64 + e[k]).clamp(0.0, Q).round() as u16;
                }
                let c = cache.get(self, v);
                let (fwd, back) = if ltr { (x + 2, x) } else { (x, x + 2) };
                for k in 0..nch {
                    let err = v[k] as f64 - c[k] as f64;
                    cur[fwd][k] += err * 7.0 / 16.0;
                    next[back][k] += err * 3.0 / 16.0;
                    next[x + 1][k] += err * 5.0 / 16.0;
                    next[fwd][k] += err / 16.0;
                }
                img.pixels[at..at + nch].copy_from_slice(&c[..nch]);
            }
            std::mem::swap(&mut cur, &mut next);
            next.iter_mut().for_each(|e| *e = [0.0; 4]);
        }
    }

    fn riemersma(&self, img: &mut Image, cache: &mut DitherCache) {
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
            errors: [[0.0; 4]; 16],
            weights,
            cache,
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
    slot: std::collections::HashMap<u32, [u16; 4]>,
    nch: usize,
}

impl DitherCache {
    fn new(nch: usize) -> DitherCache {
        DitherCache {
            slot: std::collections::HashMap::new(),
            nch,
        }
    }
    fn get(&mut self, cube: &Cube, v: [u16; 4]) -> [u16; 4] {
        let key = (0..self.nch).fold(0u32, |k, c| k | ((char8(v[c]) >> 2) << (6 * c)));
        *self.slot.entry(key).or_insert_with(|| cube.lookup(v))
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
    errors: [[f64; 4]; 16],
    weights: [f64; 16],
    cache: &'a mut DitherCache,
}

impl Hilbert<'_> {
    fn step(&mut self, d: Dir) {
        if self.x >= 0 && self.y >= 0 && self.x < self.w && self.y < self.h {
            let nch = self.cube.nch;
            let at = ((self.y * self.w + self.x) * 4) as usize;
            let mut v = px4(&self.img.pixels[at..at + 4], nch == 4);
            for k in 0..nch {
                let e: f64 = (0..16).map(|j| self.errors[j][k] * self.weights[j]).sum();
                v[k] = (v[k] as f64 + e).clamp(0.0, Q).round() as u16;
            }
            let c = self.cache.get(self.cube, v);
            self.errors.copy_within(1.., 0);
            let mut err = [0.0; 4];
            for k in 0..nch {
                err[k] = v[k] as f64 - c[k] as f64;
            }
            self.errors[15] = err;
            self.img.pixels[at..at + nch].copy_from_slice(&c[..nch]);
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

fn unique_colors(imgs: &[&Image], alpha: bool, limit: usize) -> Option<usize> {
    let mut seen = std::collections::HashSet::new();
    for i in imgs {
        for p in i.pixels.chunks_exact(4) {
            seen.insert(px4(p, alpha));
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
    let alpha = imgs.iter().any(|i| i.alpha);
    {
        let all: Vec<&Image> = imgs.iter().map(|i| &**i).collect();
        if let Some(n) = unique_colors(&all, alpha, colors) {
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
    let mut cube = Cube::new(depth, alpha);
    for i in imgs.iter() {
        cube.add_image(i);
    }
    cube.reduce(colors);
    let n = cube.palette().len();
    let mut cache = DitherCache::new(cube.nch);
    for i in imgs.iter_mut() {
        cube.assign_with(i, dither, &mut cache);
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
    let mut cube = Cube::new(8, false);
    let mut seen = std::collections::HashSet::new();
    let mut gray = true;
    for p in reference.pixels.chunks_exact(4) {
        let c = px4(p, false);
        if seen.insert(c) {
            gray &= c[0] == c[1] && c[1] == c[2];
            cube.add(c, 1.0);
        }
    }
    if seen.is_empty() {
        cube.add([0, 0, 0, 65535], 1.0);
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
    let mut cube = Cube::new(8, false);
    for c in palette {
        cube.add([c[0], c[1], c[2], 65535], 1.0);
    }
    cube.reduce(usize::MAX);
    cube.assign(img, dither);
}

/// KmeansImage(): Lloyd iterations from `colors` centres: the `seeds`
/// first, the rest from the colour map of an undithered quantization
/// (ImageMagick picks random image colours for unseeded centres). Each iteration assigns
/// every pixel its closest centre (RGB) and moves the centres to the
/// means; it stops after `iterations` or once the total squared
/// distance (QuantumScale units) changes by no more than `tolerance`.
/// Pixels take their centre's rounded colour.
pub fn kmeans(
    img: &mut Image,
    colors: usize,
    iterations: usize,
    tolerance: f64,
    seeds: Option<Vec<[u16; 3]>>,
) {
    if iterations == 0 || img.pixels.is_empty() {
        return;
    }
    let colors = colors.max(1);
    let mut centres: Vec<[f64; 3]> = seeds
        .unwrap_or_default()
        .iter()
        .take(colors)
        .map(|c| c.map(|v| v as f64))
        .collect();
    if centres.len() < colors {
        let mut q = img.clone();
        q.alpha = false;
        quantize(&mut q, colors, 0, Dither::None);
        let mut seen = std::collections::HashSet::new();
        let mut pal: Vec<[u16; 3]> = q
            .pixels
            .chunks_exact(4)
            .map(|p| [p[0], p[1], p[2]])
            .filter(|c| seen.insert(*c))
            .collect();
        pal.sort_unstable();
        let missing = colors - centres.len();
        centres.extend(pal.iter().take(missing).map(|c| c.map(|v| v as f64)));
    }
    // the pixels as a histogram, in first-seen order
    let mut index = std::collections::HashMap::new();
    let mut uniq: Vec<([f64; 3], f64)> = vec![];
    let mut of_pixel = Vec::with_capacity(img.pixels.len() / 4);
    for p in img.pixels.chunks_exact(4) {
        let k = [p[0], p[1], p[2]];
        let i = *index.entry(k).or_insert_with(|| {
            uniq.push((k.map(|v| v as f64), 0.0));
            uniq.len() - 1
        });
        uniq[i].1 += 1.0;
        of_pixel.push(i as u32);
    }
    let mut label = vec![0usize; uniq.len()];
    let mut previous: Option<f64> = None;
    for _ in 0..iterations {
        let mut total = 0.0;
        let mut sums = vec![[0f64; 3]; centres.len()];
        let mut counts = vec![0f64; centres.len()];
        for (u, (c, n)) in uniq.iter().enumerate() {
            let (mut best, mut bd) = (0usize, f64::MAX);
            for (j, m) in centres.iter().enumerate() {
                let d = (c[0] - m[0]).powi(2) + (c[1] - m[1]).powi(2) + (c[2] - m[2]).powi(2);
                if d < bd {
                    bd = d;
                    best = j;
                }
            }
            label[u] = best;
            total += n * bd / (Q * Q);
            counts[best] += n;
            for k in 0..3 {
                sums[best][k] += n * c[k];
            }
        }
        for (j, m) in centres.iter_mut().enumerate() {
            if counts[j] > 0.0 {
                for k in 0..3 {
                    m[k] = sums[j][k] / counts[j];
                }
            }
        }
        if previous.is_some_and(|p| (total - p).abs() <= tolerance) {
            break;
        }
        previous = Some(total);
    }
    let out: Vec<[u16; 3]> = centres
        .iter()
        .map(|m| m.map(|v| (v + 0.5).floor().clamp(0.0, Q) as u16))
        .collect();
    for (p, &u) in img.pixels.chunks_exact_mut(4).zip(&of_pixel) {
        p[..3].copy_from_slice(&out[label[u as usize]]);
    }
    img.colors = None;
}
