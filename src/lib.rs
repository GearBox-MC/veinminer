//! Vein Miner: hold the vein key (default `) while breaking a block and the connected blocks of
//! the same kind break with it, FTB Ultimine style. Behaviour-only reimplementation for Horizon.
//!
//! - Client: a registered key mapping; its held state is sent to the server on change (and
//!   refreshed once a second while held). A small HUD line shows the vein size while held.
//! - Server: `break_block` runs the vein through `PlayerRef::destroy_block`, so every block is a
//!   real survival break (tool wear, exhaustion, game mode checks). `block_drops` keeps the
//!   drops and experience of the vein's breaks and they are set down merged at the origin.

mod vein;

use mod_api::*;
use std::collections::{HashMap, HashSet};

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

pub const MOD_ID: &str = "gearbox_veinminer";
const KEY_ID: &str = "activate";
const CYCLE_ID: &str = "cycle_shape";

const DEFAULT_CONFIG: &str = r#"# Vein Miner settings. Changes are picked up while the game runs.

# Most blocks one vein may break, the mined block included (1 to 1024).
max_blocks = 64

# When felling a tree, also break the leaves touching it.
include_leaves = false

# Hunger: exhaustion added for every block of a vein beyond the first, on top of vanilla's
# 0.005 per mined block (4.0 exhaustion = one saturation point, then one hunger point).
# 0 turns it off. Creative players are exempt.
exhaustion_per_block = 0.1

# In survival, a vein does not start, and stops partway, when the food level (0 to 20) is at
# or below this value. 0: only an empty hunger bar stops it.
min_food_level = 0

# Tell the player in chat what each vein did (blocks, tool damage, food before and after).
debug = false

# Shape modes (everything but Shapeless) break only the blocks that match the mined block,
# the same rule as Shapeless; the shape only limits where. Set true to break any breakable
# block in the shape instead (skipping air, fluids, unbreakables and, when the held tool is
# a proper tool for the mined block, blocks it is not fast on).
shape_any_block = false

# Block tags whose members count as one vein (a block always joins blocks of its own id).
# Vanilla's per-ore tags group stone and deepslate ores; the c:ores/<name> tags, which
# Horizon's own data does not have, pick up modded ores when a mod or data pack adds them.
group_tags = ["minecraft:coal_ores", "minecraft:iron_ores", "minecraft:copper_ores", "minecraft:gold_ores", "minecraft:redstone_ores", "minecraft:lapis_ores", "minecraft:diamond_ores", "minecraft:emerald_ores", "c:ores/coal", "c:ores/iron", "c:ores/copper", "c:ores/gold", "c:ores/redstone", "c:ores/lapis", "c:ores/diamond", "c:ores/emerald", "c:ores/quartz", "c:ores/netherite_scrap", "c:ores/tin", "c:ores/silver", "c:ores/lead", "c:ores/zinc", "c:ores/nickel", "c:ores/uranium", "c:ores/osmium", "c:ores/aluminum"]
"#;

/// The key state the client sends: whether the vein key is held, the selected shape and the
/// face of the block under the crosshair (an index into `vein::FACES`).
#[derive(PayloadCodec, Clone, Copy, PartialEq, Eq, Default)]
struct KeyState {
    held: bool,
    shape: u8,
    face: u8,
}

static KEY_TOKEN: OnceLock<ToServer<KeyState>> = OnceLock::new();

/// Settings, shared by both sides (a `Common` config).
static MAX_BLOCKS: AtomicU32 = AtomicU32::new(64);
static INCLUDE_LEAVES: AtomicBool = AtomicBool::new(false);
static SHAPE_ANY_BLOCK: AtomicBool = AtomicBool::new(false);
/// f32 bits.
static EXHAUSTION_PER_BLOCK: AtomicU32 = AtomicU32::new(0x3DCC_CCCD); // 0.1
static MIN_FOOD_LEVEL: AtomicU32 = AtomicU32::new(0);
/// `Player.causeFoodExhaustion(0.005)` in `Block.playerDestroy`, which `destroy_block` runs.
const VANILLA_EXHAUSTION_PER_BLOCK: f32 = 0.005;
static DEBUG: AtomicBool = AtomicBool::new(false);
static GROUP_TAGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn group_tags() -> Vec<String> {
    GROUP_TAGS.lock().unwrap().clone()
}

/// A one-line TOML string array: `["a", "b"]`.
fn parse_string_array(value: &str) -> Option<Vec<String>> {
    let inner = value.trim().strip_prefix('[')?.strip_suffix(']')?;
    Some(
        inner
            .split(',')
            .map(|s| s.trim().trim_matches('"').trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

/// Server: each player's (by UUID) last key state.
static PLAYERS: Mutex<Option<HashMap<u128, KeyState>>> = Mutex::new(None);

fn set_state(uuid: u128, state: Option<KeyState>) {
    let mut guard = PLAYERS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    match state {
        Some(s) => {
            map.insert(uuid, s);
        }
        None => {
            map.remove(&uuid);
        }
    }
}

fn state_of(uuid: u128) -> Option<KeyState> {
    PLAYERS.lock().unwrap().as_ref().and_then(|m| m.get(&uuid).copied())
}

fn apply_config(text: &str) {
    let mut max = 64u32;
    let mut leaves = false;
    let mut any_block = false;
    let mut exhaustion = 0.1f32;
    let mut min_food = 0u32;
    let mut debug = false;
    // A file from before `group_tags` keeps the default tags.
    let mut tags: Vec<String> = DEFAULT_CONFIG
        .lines()
        .find_map(|l| l.strip_prefix("group_tags =").and_then(parse_string_array))
        .unwrap_or_default();
    for line in text.lines() {
        // Strip a comment, but not a `#` inside a quoted string.
        let line = match line.find('#') {
            Some(i) if !line[..i].contains('"') => &line[..i],
            _ => line,
        }
        .trim();
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim();
        match key.trim() {
            "max_blocks" => {
                if let Ok(v) = value.parse::<u32>() {
                    max = v.clamp(1, 1024);
                }
            }
            "include_leaves" => leaves = value == "true",
            "shape_any_block" => any_block = value == "true",
            "exhaustion_per_block" => {
                if let Ok(v) = value.parse::<f32>() {
                    exhaustion = if v.is_finite() { v.clamp(0.0, 40.0) } else { 0.1 };
                }
            }
            "debug" => debug = value == "true",
            "min_food_level" => {
                if let Ok(v) = value.parse::<u32>() {
                    min_food = v.min(20);
                }
            }
            "group_tags" => tags = parse_string_array(value).unwrap_or_default(),
            _ => {}
        }
    }
    MAX_BLOCKS.store(max, Ordering::Relaxed);
    INCLUDE_LEAVES.store(leaves, Ordering::Relaxed);
    SHAPE_ANY_BLOCK.store(any_block, Ordering::Relaxed);
    EXHAUSTION_PER_BLOCK.store(exhaustion.to_bits(), Ordering::Relaxed);
    MIN_FOOD_LEVEL.store(min_food, Ordering::Relaxed);
    DEBUG.store(debug, Ordering::Relaxed);
    log::info!("veinminer: max_blocks = {max}, include_leaves = {leaves}, {} group tags", tags.len());
    *GROUP_TAGS.lock().unwrap() = tags;
}

#[derive(Default)]
struct VeinMiner {
    /// Server: a vein is being broken (our own breaks come back through `break_block`).
    busy: AtomicBool,
    /// Server: the positions of the running vein.
    vein: Mutex<HashSet<[i32; 3]>>,
    /// Server: the breaking player's UUID.
    breaker: Mutex<Option<u128>>,
    /// Server: drops and experience kept from the vein's breaks.
    kept: Mutex<(Vec<ItemStack>, u32)>,
    /// Client: the key state last sent, and ticks since.
    client: Mutex<ClientState>,
}

#[derive(Default)]
struct ClientState {
    /// The state last sent to the server, and ticks since.
    sent: KeyState,
    since_send: u32,
    /// The selected shape (kept for the session).
    shape: vein::Shape,
    /// The vein size under the crosshair while the key is held (HUD).
    preview: Option<usize>,
    /// The HUD line, translated, rebuilt with the preview.
    hud: String,
    /// Whether the cycle mapping was held on the previous tick (edge detection).
    cycle_was_down: bool,
}


#[mod_api::horizon_mod]
impl Hooks for VeinMiner {
    fn register(&self, r: &mut dyn Registrar, _cx: &RegistrationContext) {
        r.register_key_mapping(
            KeyRegistration::new(KEY_ID, "key.keyboard.grave.accent")
                .label("key.veinminer.activate")
                .category("key.category.veinminer")
                .context(KeyConflictContext::InGame),
        );
        // Ctrl + ` cycles the shape (the scroll wheel does too, while the vein key is held).
        r.register_key_mapping(
            KeyRegistration::new(CYCLE_ID, "key.keyboard.grave.accent")
                .label("key.veinminer.cycle_shape")
                .category("key.category.veinminer")
                .modifiers(ClientModifiers::new(2))
                .context(KeyConflictContext::InGame),
        );
        let mut payloads = r.payload_handlers();
        let mut view = payloads.registrar("1");
        let token = view.play_to_server_typed::<KeyState>(
            PayloadId("gearbox_veinminer:key".into()),
            |value, cx| {
                let player = cx.player().map_err(|e| format!("{e:?}"))?;
                set_state(player.uuid(), Some(value));
                Ok(())
            },
        );
        let _ = KEY_TOKEN.set(token);
    }

    fn register_configs(&self, _host: HostSide, event: &mut RegisterConfigsEvent<'_>) {
        event.configs.push(Config::new(MOD_ID, ConfigKind::Common, DEFAULT_CONFIG));
    }

    fn config_loading(&self, _host: HostSide, event: &mut ConfigLoadingEvent<'_>) {
        if event.config.mod_id == MOD_ID {
            apply_config(&event.config.text);
        }
    }

    fn config_reloading(&self, _host: HostSide, event: &mut ConfigReloadingEvent<'_>) {
        if event.config.mod_id == MOD_ID {
            apply_config(&event.config.text);
        }
    }

    fn player_logged_out(&self, _host: &mut dyn ServerHost, event: &mut PlayerLoggedOutEvent<'_>) {
        set_state(event.player, None);
    }

    // ---- client ----

    fn client_tick_pre(&self, host: &mut dyn ClientHost, _event: &mut ClientTickPreEvent) {
        // Before the tick's key handling, so the face reaches the server before a break starts.
        let down = host.playing() && host.is_down(MOD_ID, KEY_ID);
        let mut state = self.client.lock().unwrap();
        // One shape per press: a new press (a click, or the mapping going down because its
        // modifier joined a held key) cycles once; key repeat while it stays down does nothing.
        let mut clicked = false;
        while host.consume_click(MOD_ID, CYCLE_ID) {
            clicked = true;
        }
        let cycle_down = host.is_down(MOD_ID, CYCLE_ID);
        if (clicked || cycle_down) && !state.cycle_was_down {
            state.shape = state.shape.cycle(1);
            state.preview = None;
        }
        state.cycle_was_down = cycle_down;
        let face = match host.crosshair_target() {
            Some(CrosshairTarget::Block { hit, .. }) => vein::face_index(hit.face),
            _ => state.sent.face,
        };
        let now = KeyState { held: down, shape: state.shape as u8, face };
        state.since_send += 1;
        // Send on change, and refresh once a second while held (a lost packet or a rejoin).
        if now != state.sent || (down && state.since_send >= 20) {
            if let Some(token) = KEY_TOKEN.get() {
                if token.send(host, &now).is_ok() {
                    state.sent = now;
                    state.since_send = 0;
                }
            }
        }
        // HUD preview, every 4 ticks while held.
        if !down {
            state.preview = None;
        } else if state.since_send % 4 == 0 || state.preview.is_none() {
            state.preview = client_preview(&*host, state.shape);
            state.hud = hud_line(&*host, state.shape, state.preview);
        }
    }

    fn mouse_scrolling(&self, host: &mut dyn ClientHost, event: &mut MouseScrollingEvent) -> Flow {
        // While the vein key is held the wheel cycles shapes instead of the hotbar.
        if !(host.playing() && host.is_down(MOD_ID, KEY_ID)) {
            return Flow::Continue;
        }
        let steps = event.accumulated[1];
        if steps != 0 {
            let mut state = self.client.lock().unwrap();
            // One shape per wheel notch (wheel down = next shape).
            state.shape = state.shape.cycle(-steps);
            state.preview = None;
        }
        Flow::Cancel
    }

    fn gui_render_post(&self, _host: &dyn ClientHost, event: &mut GuiRenderPostEvent<'_>) {
        if event.gui_hidden {
            return;
        }
        let state = self.client.lock().unwrap();
        if !state.sent.held {
            return;
        }
        if state.hud.is_empty() {
            return;
        }
        let line = state.hud.as_str();
        let [w, h] = event.batch.size();
        let tw = event.batch.text_width(line);
        let x = (w - tw) / 2.0;
        let y = h / 2.0 + 12.0;
        event.batch.fill([x - 3.0, y - 2.0, tw + 6.0, 12.0], 0x8000_0000);
        event.batch.text(line, [x, y], 0xFFFF_D24A, true);
    }

    // ---- server ----

    fn break_block(&self, host: &mut dyn ServerHost, event: &mut BreakBlockEvent<'_>) -> Flow {
        // Our own breaks come through here too.
        if self.busy.load(Ordering::Relaxed) {
            return Flow::Continue;
        }
        let Some(player) = host.players().acting_player() else { return Flow::Continue };
        let uuid = player.uuid();
        let Some(key) = state_of(uuid).filter(|s| s.held) else { return Flow::Continue };
        let Some(level) = player.level(&*host) else { return Flow::Continue };
        let origin = event.pos;
        let max = MAX_BLOCKS.load(Ordering::Relaxed) as usize;
        let shape = vein::Shape::from_u8(key.shape);
        let face = vein::FACES[(key.face as usize).min(5)];
        let tool_at_start = LivingRef::new(player.entity()).hand_item(&*host, Hand::Main);
        let facing = horizontal_facing(&*host, player.entity());
        let positions =
            plan(&*host, level, origin, event.state.block(), shape, face, facing, max, tool_at_start.as_ref());
        if positions.len() <= 1 {
            return Flow::Continue; // nothing else to take: an ordinary break
        }
        if !player.is_creative(&*host).unwrap_or(false)
            && player
                .food(&*host)
                .is_some_and(|f| f.level <= MIN_FOOD_LEVEL.load(Ordering::Relaxed) as i32)
        {
            return Flow::Continue; // too hungry for a vein: an ordinary break
        }

        self.busy.store(true, Ordering::Relaxed);
        *self.vein.lock().unwrap() = positions.iter().copied().collect();
        *self.breaker.lock().unwrap() = Some(uuid);

        let food_before = player.food(&*host);
        let damage_before = tool_at_start.as_ref().map_or(0, |t| t.damage());
        let mut broken = 0usize;
        let creative = player.is_creative(&*host).unwrap_or(false);
        let min_food = MIN_FOOD_LEVEL.load(Ordering::Relaxed) as i32;
        let cost = f32::from_bits(EXHAUSTION_PER_BLOCK.load(Ordering::Relaxed));
        // The game takes exhaustion off the food bar once a tick, after the whole vein: so
        // follow where the bar will be (saturation first, then food), as `FoodData.tick` does.
        let mut predicted = food_before.map(|f| (f.level, f.saturation, f.exhaustion));
        let spend = |predicted: &mut Option<(i32, f32, f32)>, amount: f32| {
            if let Some((level, sat, exh)) = predicted.as_mut() {
                *exh = (*exh + amount).min(40.0);
                while *exh > 4.0 {
                    *exh -= 4.0;
                    if *sat > 0.0 {
                        *sat = (*sat - 1.0).max(0.0);
                    } else {
                        *level = (*level - 1).max(0);
                    }
                }
            }
        };
        for (i, pos) in positions.iter().enumerate() {
            if i > 0 && tool_broke(&tool_at_start, &LivingRef::new(player.entity()).hand_item(&*host, Hand::Main)) {
                break; // the tool broke on the previous block: stop
            }
            if i > 0 && !creative && predicted.is_some_and(|(level, _, _)| level <= min_food) {
                break; // too hungry to go on: the rest stays
            }
            if player.destroy_block(host, *pos, false) {
                broken += 1;
                // Our cost for each block beyond the first, on top of vanilla's 0.005 per block
                // that `destroy_block` already causes. Creative gets none (the game skips it).
                spend(&mut predicted, VANILLA_EXHAUSTION_PER_BLOCK);
                if i > 0 && cost > 0.0 {
                    player.add_exhaustion(host, cost);
                    spend(&mut predicted, cost);
                }
            } else if i == 0 {
                break; // the origin itself was refused: let nothing else go
            }
        }

        self.busy.store(false, Ordering::Relaxed);
        self.vein.lock().unwrap().clear();
        *self.breaker.lock().unwrap() = None;
        let (stacks, experience) = std::mem::take(&mut *self.kept.lock().unwrap());
        let kept_items: u32 = stacks.iter().map(|s| s.count()).sum();
        let merged = merge(&*host, stacks);
        let mut popped = Vec::new();
        for stack in merged {
            let count = stack.count();
            let ok = matches!(level.pop_resource(host, origin, stack), Ok(Some(_)));
            popped.push((count, ok));
        }
        if experience > 0 {
            let [x, y, z] = origin.map(|v| f64::from(v) + 0.5);
            level.award_experience(host, x, y, z, experience);
        }
        log::info!("veinminer: {} broke {broken} of {} planned blocks", shape.name(), positions.len());
        if DEBUG.load(Ordering::Relaxed) {
            let after = player.food(&*host);
            let damage_after =
                LivingRef::new(player.entity()).hand_item(&*host, Hand::Main).map_or(0, |t| t.damage());
            let fmt = |f: Option<PlayerFood>| {
                f.map_or("?".to_owned(), |f| format!("{}/{:.2}/{:.3}", f.level, f.saturation, f.exhaustion))
            };
            let line = format!(
                "[veinminer] {}: {broken}/{} blocks, tool damage {damage_before}->{damage_after}, food/sat/exh {} -> {}, items {kept_items} in stacks {popped:?}, xp {experience}",
                shape.name(),
                positions.len(),
                fmt(food_before),
                fmt(after)
            );
            let _ = player.tell(host, Text::literal(line));
        }
        if broken == 0 {
            return Flow::Continue; // nothing went: vanilla handles the origin as usual
        }
        // The origin is already broken by the loop.
        Flow::Cancel
    }

    fn block_drops(&self, _level: &dyn LevelHost, event: &mut BlockDropsEvent<'_>) -> Flow {
        if !self.busy.load(Ordering::Relaxed) {
            return Flow::Continue;
        }
        if event.breaker.is_none() || event.breaker != *self.breaker.lock().unwrap() {
            return Flow::Continue;
        }
        let pos = [event.pos.x, event.pos.y, event.pos.z];
        if self.vein.lock().unwrap().contains(&pos) {
            let mut kept = self.kept.lock().unwrap();
            kept.0.extend(event.drops.drain(..).map(|drop| drop.stack));
            kept.1 += std::mem::take(&mut event.experience);
        }
        Flow::Continue
    }
}

/// Whether the held tool broke (or was swapped away) since the vein started.
fn tool_broke(start: &Option<ItemStack>, now: &Option<ItemStack>) -> bool {
    match (start, now) {
        (Some(a), Some(b)) => a.item() != b.item(),
        (Some(_), None) => true,
        _ => false,
    }
}

/// Folds each stack into the earlier ones it stacks with, up to the item's max stack size.
fn merge(host: &dyn Host, stacks: Vec<ItemStack>) -> Vec<ItemStack> {
    let mut out: Vec<ItemStack> = Vec::new();
    for mut stack in stacks {
        let max = host.max_stack_size(stack.item()).unwrap_or(64);
        // `set_count(0)` is refused, so count what is left apart from the stack.
        let mut left = stack.count();
        for existing in out.iter_mut() {
            if left == 0 {
                break;
            }
            if existing.is_stackable_with(&stack) && existing.count() < max {
                let moved = (max - existing.count()).min(left);
                if existing.grow(moved).is_ok() {
                    left -= moved;
                }
            }
        }
        if left > 0 && stack.set_count(left).is_ok() {
            out.push(stack);
        }
    }
    out
}

/// The blocks a vein takes, origin first, at most `max`.
#[allow(clippy::too_many_arguments)]
fn plan(
    host: &dyn Host,
    level: LevelRef,
    origin: [i32; 3],
    origin_block: BlockKey,
    shape: vein::Shape,
    face: [i32; 3],
    facing: [i32; 3],
    max: usize,
    tool: Option<&ItemStack>,
) -> Vec<[i32; 3]> {
    let tags = group_tags();
    let rules = vein::Rules::new(host, origin_block, &tags);
    if shape == vein::Shape::Shapeless {
        let leaves = INCLUDE_LEAVES.load(Ordering::Relaxed);
        return vein::find(
            origin,
            &rules,
            max,
            leaves,
            |b| host.block_in_tag(b, "minecraft:leaves").unwrap_or(false),
            |p| level.block_state(host, p).map(|s| s.block()),
        );
    }
    let any_block = SHAPE_ANY_BLOCK.load(Ordering::Relaxed);
    // With `shape_any_block`: when the tool is a proper tool for the hit block, only blocks it
    // is also fast on (no dirt in a pickaxe tunnel). Otherwise the blocks match the origin, so
    // the origin's harvestability holds for all of them.
    let tool_fits_origin =
        any_block && level.block_break_speed(host, origin, tool).is_some_and(|s| s > 1.0);
    let mut out = vec![origin];
    'layers: for (depth, layer) in
        vein::shape_layers(shape, origin, face, facing, max).into_iter().enumerate()
    {
        let before = out.len();
        for pos in layer {
            if out.len() >= max {
                break 'layers;
            }
            if pos == origin {
                continue;
            }
            let Some(state) = level.block_state(host, pos) else { continue };
            if !any_block {
                if rules.matches(state.block()) {
                    out.push(pos);
                }
                continue;
            }
            if host.block_states().replaceable(state).unwrap_or(true) {
                continue; // air, fluids, grass, snow layers
            }
            if host.block_states().destroy_speed(state).is_none_or(|h| h < 0.0) {
                continue; // bedrock and other unbreakables
            }
            if tool_fits_origin && !level.block_break_speed(host, pos, tool).is_some_and(|s| s > 1.0) {
                continue;
            }
            out.push(pos);
        }
        // A tunnel ends where nothing more is taken: it never jumps a gap.
        if depth > 0 && out.len() == before {
            break;
        }
    }
    out
}

/// The entity's horizontal facing as a unit vector, from its yaw (`Entity.getDirection`).
fn horizontal_facing(host: &dyn Host, entity: EntityRef) -> [i32; 3] {
    facing_from_yaw(entity.rotation(host).map_or(0.0, |[yaw, _]| yaw))
}

/// `Direction.fromYRot`: yaw 0 faces south, 90 west, 180 north, -90 east.
fn facing_from_yaw(yaw: f32) -> [i32; 3] {
    match ((yaw / 90.0 + 0.5).floor() as i32).rem_euclid(4) {
        0 => [0, 0, 1],
        1 => [-1, 0, 0],
        2 => [0, 0, -1],
        _ => [1, 0, 0],
    }
}

/// The HUD line in the player's language.
fn hud_line(host: &dyn ClientHost, shape: vein::Shape, count: Option<usize>) -> String {
    let shape_text = Text::translate(shape.translation_key(), vec![]);
    let text = match count {
        Some(n) => Text::translate("veinminer.hud.count", vec![shape_text, Text::literal(n.to_string())]),
        None => Text::translate("veinminer.hud.shape", vec![shape_text]),
    };
    host.text_string(&text).unwrap_or_else(|_| format!("Vein [{}]", shape.name()))
}

/// The vein size under the crosshair, read from the client's world.
fn client_preview(host: &dyn ClientHost, shape: vein::Shape) -> Option<usize> {
    let CrosshairTarget::Block { pos, state, hit } = host.crosshair_target()? else { return None };
    let player = host.local_player()?;
    let level = player.level(host)?;
    let tool = LivingRef::new(player.entity()).hand_item(host, Hand::Main);
    let max = MAX_BLOCKS.load(Ordering::Relaxed) as usize;
    // The camera's yaw is the look the player sees (the entity's can trail it on the client).
    let facing = host
        .camera()
        .map_or_else(|| horizontal_facing(host, player.entity()), |c| facing_from_yaw(c.yaw));
    let found =
        plan(host, level, [pos.x, pos.y, pos.z], state.block(), shape, hit.face, facing, max, tool.as_ref());
    Some(found.len())
}
