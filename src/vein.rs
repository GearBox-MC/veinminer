//! Finding the blocks of a vein: the same-block flood fill and the fixed shapes.

use mod_api::{BlockKey, Host};
use std::collections::{HashSet, VecDeque};

/// What counts as the same block as the origin: the block itself, and every block sharing one of
/// the configured grouping tags with it (`minecraft:iron_ores` puts iron ore and deepslate iron
/// ore in one vein; a mod's ores join through its `c:ores/<name>` tags).
pub struct Rules<'h> {
    host: &'h dyn Host,
    origin: BlockKey,
    /// The grouping tags the origin is in.
    tags: Vec<String>,
    /// The origin is a log: leaves may join (when enabled).
    pub is_log: bool,
}

impl<'h> Rules<'h> {
    pub fn new(host: &'h dyn Host, origin: BlockKey, group_tags: &[String]) -> Self {
        let tags = group_tags
            .iter()
            .filter(|t| host.block_in_tag(origin, t).unwrap_or(false))
            .cloned()
            .collect();
        let is_log = host.block_in_tag(origin, "minecraft:logs").unwrap_or(false);
        Self { host, origin, tags, is_log }
    }

    pub fn matches(&self, block: BlockKey) -> bool {
        block == self.origin
            || self.tags.iter().any(|t| self.host.block_in_tag(block, t).unwrap_or(false))
    }
}

/// The 26 neighbour offsets, faces first, then edges, then corners (nearest first).
fn neighbours() -> Vec<[i32; 3]> {
    let mut out = Vec::with_capacity(26);
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                if (dx, dy, dz) != (0, 0, 0) {
                    out.push([dx, dy, dz]);
                }
            }
        }
    }
    out.sort_by_key(|[x, y, z]| x * x + y * y + z * z);
    out
}

fn dist2(a: [i32; 3], b: [i32; 3]) -> i64 {
    (0..3).map(|i| i64::from(a[i] - b[i]).pow(2)).sum()
}

/// Shapeless: a breadth-first flood over the 26 neighbours from `origin` of blocks the rules
/// match, at most `max`, the origin first, nearest first. With `include_leaves` and a log origin,
/// the leaves touching the trunk (and touching those leaves) fill what the cap leaves over.
pub fn find(
    origin: [i32; 3],
    rules: &Rules,
    max: usize,
    include_leaves: bool,
    is_leaves: impl Fn(BlockKey) -> bool,
    mut block_at: impl FnMut([i32; 3]) -> Option<BlockKey>,
) -> Vec<[i32; 3]> {
    let offsets = neighbours();
    let mut seen: HashSet<[i32; 3]> = HashSet::from([origin]);
    let mut found = vec![origin];
    let mut queue = VecDeque::from([origin]);
    let mut leaf_edge: Vec<[i32; 3]> = Vec::new();
    // Bound the search itself, so a huge area of one block is cheap.
    let mut budget = max.saturating_mul(32).max(256);
    'outer: while let Some(pos) = queue.pop_front() {
        let mut next: Vec<[i32; 3]> = Vec::new();
        for o in &offsets {
            let p = [pos[0] + o[0], pos[1] + o[1], pos[2] + o[2]];
            if !seen.insert(p) {
                continue;
            }
            budget = budget.saturating_sub(1);
            if budget == 0 {
                break 'outer;
            }
            match block_at(p) {
                Some(b) if rules.matches(b) => next.push(p),
                Some(b) if include_leaves && rules.is_log && is_leaves(b) => leaf_edge.push(p),
                _ => {}
            }
        }
        // Nearest to the origin first.
        next.sort_by_key(|p| dist2(*p, origin));
        for p in next {
            if found.len() >= max {
                break 'outer;
            }
            found.push(p);
            queue.push_back(p);
        }
    }
    if include_leaves && rules.is_log && found.len() < max {
        // Leaves: flood through leaves only, so a neighbouring tree's trunk is not taken.
        leaf_edge.sort_by_key(|p| dist2(*p, origin));
        let mut queue: VecDeque<[i32; 3]> = VecDeque::new();
        for p in leaf_edge {
            if found.len() >= max {
                break;
            }
            found.push(p);
            queue.push_back(p);
        }
        'leaves: while let Some(pos) = queue.pop_front() {
            for o in &offsets[..6] {
                let p = [pos[0] + o[0], pos[1] + o[1], pos[2] + o[2]];
                if !seen.insert(p) {
                    continue;
                }
                if block_at(p).is_some_and(&is_leaves) {
                    if found.len() >= max {
                        break 'leaves;
                    }
                    found.push(p);
                    queue.push_back(p);
                }
            }
        }
    }
    found
}

/// The mining shapes, cycled with the scroll wheel (or the cycle key) while the vein key is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Shape {
    /// Connected blocks of the same kind.
    #[default]
    Shapeless = 0,
    /// 1 wide, 2 tall (the block and the one below it), going into the face.
    SmallTunnel = 1,
    /// A 3x3 square on the face, going into the face.
    Tunnel3 = 2,
    /// A 3x3 square whose bottom row is the hit block, going into the face (walk-in tunnel).
    MiningTunnel = 3,
    /// A flat 3x3 square on the face, one layer deep.
    Square3 = 4,
    /// A flat 5x5 square on the face, one layer deep.
    Square5 = 5,
    /// Escape tunnel: 1 wide, 2 tall, rising one block per step in the horizontal look direction.
    StairsUp = 6,
    /// 1 wide, 2 tall, descending one block per step in the horizontal look direction.
    StairsDown = 7,
}

impl Shape {
    pub const ALL: [Shape; 8] = [
        Shape::Shapeless,
        Shape::SmallTunnel,
        Shape::Tunnel3,
        Shape::MiningTunnel,
        Shape::Square3,
        Shape::Square5,
        Shape::StairsUp,
        Shape::StairsDown,
    ];

    pub fn from_u8(v: u8) -> Shape {
        Self::ALL.get(v as usize).copied().unwrap_or_default()
    }

    pub fn name(self) -> &'static str {
        match self {
            Shape::Shapeless => "Shapeless",
            Shape::SmallTunnel => "Small tunnel 1x2",
            Shape::Tunnel3 => "3x3 tunnel",
            Shape::MiningTunnel => "Mining tunnel",
            Shape::Square3 => "Square 3x3",
            Shape::Square5 => "Square 5x5",
            Shape::StairsUp => "Escape tunnel / Staircase up",
            Shape::StairsDown => "Staircase down",
        }
    }

    /// The translation key of the shape's name.
    pub fn translation_key(self) -> &'static str {
        match self {
            Shape::Shapeless => "veinminer.shape.shapeless",
            Shape::SmallTunnel => "veinminer.shape.small_tunnel",
            Shape::Tunnel3 => "veinminer.shape.tunnel_3x3",
            Shape::MiningTunnel => "veinminer.shape.mining_tunnel",
            Shape::Square3 => "veinminer.shape.square_3x3",
            Shape::Square5 => "veinminer.shape.square_5x5",
            Shape::StairsUp => "veinminer.shape.stairs_up",
            Shape::StairsDown => "veinminer.shape.stairs_down",
        }
    }

    /// The next (`step` > 0) or previous shape.
    pub fn cycle(self, step: i32) -> Shape {
        let n = Self::ALL.len() as i32;
        Self::ALL[((self as i32 + step).rem_euclid(n)) as usize]
    }
}

/// The face directions in index order (down, up, north, south, west, east).
pub const FACES: [[i32; 3]; 6] = [[0, -1, 0], [0, 1, 0], [0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]];

pub fn face_index(face: [i32; 3]) -> u8 {
    FACES.iter().position(|f| *f == face).unwrap_or(1) as u8
}

/// The candidate positions of a fixed shape, layer by layer into the face (for the staircases:
/// step by step along `facing`, the player's horizontal look direction), each layer
/// centre-out. `face` is the hit face's outward normal.
pub fn shape_layers(
    shape: Shape,
    origin: [i32; 3],
    face: [i32; 3],
    facing: [i32; 3],
    max: usize,
) -> Vec<Vec<[i32; 3]>> {
    if let Shape::StairsUp | Shape::StairsDown = shape {
        let rise = if shape == Shape::StairsUp { 1 } else { -1 };
        // Each step: the block and the one above it, one further along and one up (or down).
        return (0..max as i32)
            .map(|i| {
                let base = [origin[0] + facing[0] * i, origin[1] + rise * i, origin[2] + facing[2] * i];
                vec![base, [base[0], base[1] + 1, base[2]]]
            })
            .collect();
    }
    let into = face.map(|v| -v);
    let (u, v) = if face[1] != 0 {
        ([1, 0, 0], [0, 0, 1])
    } else if face[0] != 0 {
        ([0, 0, 1], [0, 1, 0])
    } else {
        ([1, 0, 0], [0, 1, 0])
    };
    // (u, v) offsets within one layer.
    let (cells, depth): (Vec<(i32, i32)>, usize) = match shape {
        Shape::Shapeless => return vec![vec![origin]],
        Shape::SmallTunnel => (vec![(0, 0), (0, -1)], max),
        Shape::Tunnel3 => (square(1), max),
        Shape::MiningTunnel => (square(1).into_iter().map(|(a, b)| (a, b + 1)).collect(), max),
        Shape::Square3 => (square(1), 1),
        Shape::Square5 => (square(2), 1),
        Shape::StairsUp | Shape::StairsDown => unreachable!(),
    };
    (0..depth as i32)
        .map(|d| {
            cells
                .iter()
                .map(|&(a, b)| {
                    [
                        origin[0] + into[0] * d + u[0] * a + v[0] * b,
                        origin[1] + into[1] * d + u[1] * a + v[1] * b,
                        origin[2] + into[2] * d + u[2] * a + v[2] * b,
                    ]
                })
                .collect()
        })
        .collect()
}

/// A square of radius `r`, centre first, then nearest first.
fn square(r: i32) -> Vec<(i32, i32)> {
    let mut cells: Vec<(i32, i32)> = (-r..=r).flat_map(|a| (-r..=r).map(move |b| (a, b))).collect();
    cells.sort_by_key(|(a, b)| a * a + b * b);
    cells
}
