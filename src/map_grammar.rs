#[allow(unused_imports)]
use crate::prelude::*;
// 32 base layout formulas. A seed supplies parameters; there are no variant IDs.
pub fn random(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut n = *state;
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
    n ^ (n >> 31)
}
pub fn style(f: usize) -> usize {
    [
        0, 0, 0, 1, 1, 2, 2, 3, 4, 4, 6, 1, 2, 2, 4, 4, 4, 1, 1, 1, 6, 6, 5, 2, 0, 0, 4, 1, 1, 4,
        3, 5,
    ][f]
}

#[inline(never)]
pub fn choose(counts: &[usize; 32], masks: [u32; 3], avoid: usize, state: &mut u64) -> usize {
    let start = random(state) as usize & 31;
    let stride = (random(state) as usize & 15) * 2 + 1;
    let mut best = (usize::MAX, 0);
    for k in 0..32 {
        let f = (start + k * stride) & 31;
        let group = 1 << style(f);
        if style(f) == avoid {
            continue;
        }
        let score = counts[f] * 2
            + usize::from(masks[2] & group != 0) * 8
            + usize::from(masks[1] & group != 0) * 10000
            + usize::from(masks[0] & (1 << f) != 0) * 100000;
        if score < best.0 {
            best = (score, f);
        }
    }
    best.1
}

fn axis(n: usize, step: usize, state: &mut u64) -> [usize; 64] {
    let mut out = [0; 64];
    let (mut start, mut band) = (0, 0);
    let mut span = step - 1 + random(state) as usize % 3;
    let phase = random(state) as usize % step;
    for (i, v) in out.iter_mut().take(n).enumerate() {
        if i >= start + span {
            start += span;
            band += step;
            span = step - 1 + random(state) as usize % 3;
        }
        *v = band + (i - start) * step / span + phase;
    }
    out
}

fn connect(cells: &mut [u8], w: usize, h: usize) {
    let mut seen = vec![false; cells.len()];
    let mut q = vec![w + 1];
    cells[w + 1] = 0;
    seen[w + 1] = true;
    let mut head = 0;
    loop {
        while head < q.len() {
            let at = q[head];
            head += 1;
            let (x, z) = (at % w, at / w);
            for (dx, dz) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
                let (u, v) = (x as i32 + dx, z as i32 + dz);
                if u < 0 || v < 0 || u >= w as i32 || v >= h as i32 {
                    continue;
                }
                let n = v as usize * w + u as usize;
                if !seen[n] && cells[n] == 0 {
                    seen[n] = true;
                    q.push(n)
                }
            }
        }
        let Some(a) = (0..cells.len()).find(|&i| cells[i] == 0 && !seen[i]) else {
            break;
        };
        let b = *q
            .iter()
            .min_by_key(|&&b| (a % w).abs_diff(b % w) + (a / w).abs_diff(b / w))
            .unwrap();
        let (mut x, mut z) = (a % w, a / w);
        while x != b % w {
            cells[z * w + x] = 0;
            x = if x < b % w { x + 1 } else { x - 1 };
        }
        while z != b / w {
            cells[z * w + x] = 0;
            z = if z < b / w { z + 1 } else { z - 1 };
        }
        cells[z * w + x] = 0;
        seen.fill(false);
        q.clear();
        q.push(w + 1);
        seen[w + 1] = true;
        head = 0;
    }
}
fn maze(cells: &mut [u8], w: usize, h: usize, state: &mut u64) {
    cells.fill(1);
    cells[w + 1] = 0;
    let mut stack = vec![[1usize, 1]];
    while let Some(&at) = stack.last() {
        let mut choices = [[0; 2]; 4];
        let mut n = 0;
        for [dx, dz] in [[0, -2], [2, 0], [0, 2], [-2, 0]] {
            let (x, z) = (at[0] as i32 + dx, at[1] as i32 + dz);
            if x > 0
                && z > 0
                && x < w as i32 - 1
                && z < h as i32 - 1
                && cells[z as usize * w + x as usize] != 0
            {
                choices[n] = [x as usize, z as usize];
                n += 1;
            }
        }
        if n == 0 {
            stack.pop();
        } else {
            let p = choices[random(state) as usize % n];
            cells[(p[1] + at[1]) / 2 * w + (p[0] + at[0]) / 2] = 0;
            cells[p[1] * w + p[0]] = 0;
            stack.push(p);
        }
    }
}
pub fn tile(w: usize, h: usize, family: usize, seed: u64) -> Vec<u8> {
    assert!(w <= 64 && h <= 64);
    let mut state = seed;
    let p = 4 + random(&mut state) as usize % 5;
    let q = 4 + random(&mut state) as usize % 5;
    let cx = w / 3 + random(&mut state) as usize % (w / 3);
    let cz = h / 3 + random(&mut state) as usize % (h / 3);
    let door = 1 + random(&mut state) as usize % (p.min(q) - 3).min(3);
    let thick = 1 + random(&mut state) as usize % 2;
    let rim = 2 + random(&mut state) as usize % 3;
    let skew = 1 + random(&mut state) as usize % 3;
    let xs = axis(w, p, &mut state);
    let zs = axis(h, q, &mut state);
    // Each row/column has independent geometry parameters, not one global offset.
    let mut cuts = [0usize; 128];
    for n in cuts.iter_mut().take(w + h) {
        *n = random(&mut state) as usize;
    }
    let mut cells = vec![0u8; w * h];
    if matches!(family, 0 | 1 | 2 | 24 | 25) {
        maze(&mut cells, w, h, &mut state);
    }
    for z in 0..h {
        for x in 0..w {
            let (u, v) = (xs[x], zs[z]);
            let (ux, vz) = (u % p, v % q);
            let (ix, iz) = (u / p, v / q);
            let row = cuts[w + iz % h];
            let col = cuts[ix % w];
            let gate_x = (x + row % p) % p < door;
            let gate_z = (z + col % q) % q < door;
            let vx = ux < thick && !gate_z;
            let hz = vz < thick && !gate_x;
            let inner = x > 1 && z > 1 && x + 2 < w && z + 2 < h;
            let side = if col & 1 == 0 {
                z + 2 + col % 4 < h
            } else {
                z > 1 + col % 4
            };
            let ring = x >= rim
                && z >= rim
                && x + rim < w
                && z + rim < h
                && (x == rim || z == rim || x + rim + 1 == w || z + rim + 1 == h);
            let radius = x.min(w - 1 - x).min(z.min(h - 1 - z));
            let radial = radius % (3 + door) == cuts[radius / (3 + door)] % (3 + door);
            let dx = x.abs_diff(cx);
            let dz = z.abs_diff(cz);
            let offset = (row % 3 + iz % 2 * (p / 2)) % p;
            let pillars = (u + offset) % p < 1 + col % thick && (v + col % 2) % q < 1 + row % thick;
            let base = cells[z * w + x] != 0;
            let solid = match family {
                0 => base,
                1 => {
                    base && !(x > 1
                        && x + 2 < w
                        && z > 1
                        && z + 2 < h
                        && ((x.wrapping_add(row)) % p == 0 || (z.wrapping_add(col)) % q == 0))
                }
                2 => base && dz >= door,
                3 => ux < thick && side,
                4 => hz || (dx < thick && z > rim && z + rim < h && !gate_z),
                5 => vx || hz,
                6 => ((u + offset) % p < thick && !gate_z) || hz,
                7 => {
                    inner
                        && ((dx == p / 2 && dz > q / 2 && z.abs_diff(col % h) >= door)
                            || (dz == q / 2 && dx > p / 2 && x.abs_diff(row % w) >= door))
                }
                8 => {
                    (ring && dx >= door && dz >= door)
                        || (vx && x > rim && x + rim < w && z > rim && z + rim < h)
                }
                9 => {
                    (ring && dx >= door && dz >= door)
                        || (x == cx && z > rim && z + rim < h && !gate_z)
                }
                10 => inner && ux >= door && vz >= door && !(ux >= p - 2 && vz >= q - 2),
                11 => {
                    vz < thick
                        && if row & 1 == 0 {
                            x + 2 + row % 4 < w
                        } else {
                            x > 1 + row % 4
                        }
                }
                12 => (vx && dz > door) || (vz < thick && ux > door),
                13 => (vx || hz) && dz >= door,
                14 => {
                    (ring && dx >= door && dz >= door)
                        || (radius == rim + 3 && dx >= door && dz >= door)
                        || (radius > rim + 3 && pillars)
                }
                15 => {
                    radial
                        && !match (radius / (3 + door) + skew) % 4 {
                            0 => x < cx && z < cz,
                            1 => x > cx && z < cz,
                            2 => x > cx && z > cz,
                            _ => x < cx && z > cz,
                        }
                }
                16 => radial && dx >= door && dz >= door,
                17 => hz && x > 1 + row % 3 && x + 2 + col % 3 < w,
                18 => vx || (vz == q - 1 && ux < door + 2 && !gate_x),
                19 => inner && ux >= door && dz >= door && vz >= q - door,
                20 => inner && pillars,
                21 => inner && ((u + offset) % p < thick && (v + col % q) % q < thick + 1),
                22 => {
                    inner
                        && if (row ^ col) & 1 == 0 {
                            ux < thick && vz < q / 2 || vz < thick && ux < p / 2
                        } else {
                            ux >= p - thick && vz >= q / 2 || vz >= q - thick && ux >= p / 2
                        }
                }
                23 => (vx && (iz % 2 == 0 || ux == 0)) || (hz && (ix % 2 == 1 || vz == 0)),
                24 => base && radius >= door,
                25 => base && !(dx < p / 2 && dz < q / 2),
                26 => {
                    (ring && dx >= door && dz >= door)
                        || (inner && (x + z * skew) % p < thick && !gate_z)
                }
                27 => inner && dz >= door && ux >= door && ux + 2 < p,
                28 => inner && (v + u * skew / p) % q < thick && !gate_x,
                29 => {
                    inner
                        && dx.max(dz) % (3 + door) == rim % (3 + door)
                        && if dx >= dz {
                            z.abs_diff(2 + col % (h - 4)) >= door
                        } else {
                            x.abs_diff(2 + row % (w - 4)) >= door
                        }
                }
                30 => (dx < thick && dz >= door) || (inner && pillars && (ix + iz) % 2 == 0),
                _ => inner && ((vx && row & 1 == 0) || (hz && col & 1 == 0)),
            };
            cells[z * w + x] = solid as u8;
        }
    }
    connect(&mut cells, w, h);
    cells
}

/// A spatial blend of two independently parameterized formulas, never a pair table.
pub fn blend(w: usize, h: usize, first: usize, second: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    let aseed = random(&mut state);
    let bseed = random(&mut state);
    let mut a = tile(w, h, first, aseed);
    let b = tile(w, h, second, bseed);
    let mode = random(&mut state) % 3;
    let split = w / 3 + random(&mut state) as usize % (w / 3);
    let bend = random(&mut state) as i32 % 5 - 2;
    let ix = w / 6 + random(&mut state) as usize % 2;
    let iz = h / 6 + random(&mut state) as usize % 2;
    let split_z = h / 3 + random(&mut state) as usize % (h / 3);
    for z in 0..h {
        for x in 0..w {
            let use_second = match mode {
                0 => x >= (split as i32 + bend * z as i32 / h as i32) as usize,
                1 => x >= ix && x + ix < w && z >= iz && z + iz < h,
                _ => (x >= split) != (z >= split_z),
            };
            if use_second {
                a[z * w + x] = b[z * w + x];
            }
        }
    }
    connect(&mut a, w, h);
    a
}
