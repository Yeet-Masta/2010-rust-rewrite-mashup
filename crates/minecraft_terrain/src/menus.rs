//! Menus on the integrated server (26.3 `ServerPlayer.openMenu` and
//! `doCloseContainer`, the blocks' `useWithoutItem`, `broadcastChanges`
//! and the menu packets of `ServerGamePacketListenerImpl`).
//!
//! A use of a block that opens a menu comes with the player
//! ([`PlayerContext`]), and is always answered ([`UseResult`]): the block's
//! rules decide whether it opens. An open menu runs on the menu engine
//! (`minecraftoss_player::menu`) over the block's own storage, which it
//! reads before and writes back after every batch of inputs, through the
//! level's container API, so comparators, hoppers and saves see what the
//! player did.
//!
//! The player's inventory stays the client's. Every batch comes with a copy
//! of it, taken when the batch is sent, and its answer lists the slots the
//! batch changed in that copy, with the cursor. The client keeps one batch
//! in flight and changes its inventory only from these answers, so each copy
//! is current. A tick that changed what the menu shows (a hopper feeding the
//! chest on screen) sends it again with no player part. `stillValid` is
//! checked every tick: once it fails the container closes (its sound and
//! lid), and the client is asked to send `Close`, whose answer returns the
//! carried stack.
//!
//! A furnace's menu also shows its block entity's data values (its progress),
//! read again with its slots, and a take from its result has the block
//! entity pay the recipes' experience (`awardUsedRecipesAndPopExperience`).
//! The level's furnaces cook from the recipe book ([`BookCooking`]).
//!
//! A crafter's menu shows its slots' states and `triggered` as its data
//! values, and what its grid crafts as a tenth slot, read with the grid;
//! a slot toggled in it is toggled in the block entity. The level's
//! crafters craft from the recipe book too.
//!
//! A villager's trading screen ([`MenuKind::Merchant`]) opens from the
//! use of the villager (`openTradingScreen`). Its own slots are the menu's
//! (`MerchantContainer`), and it shows the villager's offers, which it reads
//! before and after every batch and tick: what the inputs told the villager
//! (its yes and no, the trades) is told it then, and the screen is sent the
//! offers with its slots ([`MenuExtra::Merchant`]).
//!
//! A container generation left with a loot table is filled from it as it
//! opens (`unpackLootTable`), each half of a double chest in turn, with the
//! player's luck, which is always 0 here (no luck effects).
//!
//! Not simulated: piglins angered by an opening, the game events sculk
//! sensors hear, the loot advancement trigger, and spectators.

use crate::server::ServerSim;
use crate::stacks::{self, LevelStack, PlayerStack};
use minecraftoss_core::nbt::Tag;
use minecraftoss_core::BlockStateId;
use minecraftoss_entities::merchant::{MerchantMenu, Offers};
use minecraftoss_entities::tempt::PlayerCandidate;
use minecraftoss_player::inventory::Inventory;
use minecraftoss_player::crafting::{CookingKind, RecipeBook};
use minecraftoss_player::menu::{self, BlockRequest, ChestMenu, CrafterMenu, DispenserMenu, FurnaceMenu, HopperMenu, Menu, MenuContext, MenuPlace, ShulkerBoxMenu};
pub use minecraftoss_player::menu::{ContainerInput, MenuInput};
use minecraftoss_player::rng::LegacyRandom;
use minecraftoss_world::level::container::{ContainerRef, Store};
use minecraftoss_world::level::crafter::Crafting;
use minecraftoss_world::level::furnace::{Cooking, CookingRecipe, CookingType};
use minecraftoss_world::level::openers::ContainerUser;
use minecraftoss_world::level::physics::Aabb;
use minecraftoss_world::level::Level;
use serde_json::{json, Value};

type BlockPos = crate::scene::BlockPos;
type LevelPos = minecraftoss_core::BlockPos;

/// `PlayerEnderChestContainer`'s size.
pub const ENDER_SLOTS: usize = 27;

/// The kind of a menu's screen (vanilla `MenuType`). One line per kind;
/// new kinds are appended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MenuKind {
    /// `generic_9x1` to `generic_9x6` (`ChestMenu`): chests, trapped and
    /// copper chests, barrels and ender chests have 3 rows, double chests 6.
    Generic { rows: u8 },
    /// `generic_3x3` (`DispenserMenu`): dispensers and droppers.
    Generic3x3,
    /// `hopper` (`HopperMenu`).
    Hopper,
    /// `shulker_box` (`ShulkerBoxMenu`).
    ShulkerBox,
    /// `furnace` (`FurnaceMenu`).
    Furnace,
    /// `blast_furnace` (`BlastFurnaceMenu`).
    BlastFurnace,
    /// `smoker` (`SmokerMenu`).
    Smoker,
    /// `merchant` (`MerchantMenu`): a villager's trades.
    Merchant,
    /// `crafter_3x3` (`CrafterMenu`).
    Crafter,
}

impl MenuKind {
    /// The vanilla `MenuType` id.
    pub fn menu_type(self) -> &'static str {
        match self {
            Self::Generic { rows: ..=1 } => "minecraft:generic_9x1",
            Self::Generic { rows: 2 } => "minecraft:generic_9x2",
            Self::Generic { rows: 3 } => "minecraft:generic_9x3",
            Self::Generic { rows: 4 } => "minecraft:generic_9x4",
            Self::Generic { rows: 5 } => "minecraft:generic_9x5",
            Self::Generic { .. } => "minecraft:generic_9x6",
            Self::Generic3x3 => "minecraft:generic_3x3",
            Self::Hopper => "minecraft:hopper",
            Self::ShulkerBox => "minecraft:shulker_box",
            Self::Furnace => "minecraft:furnace",
            Self::BlastFurnace => "minecraft:blast_furnace",
            Self::Smoker => "minecraft:smoker",
            Self::Merchant => "minecraft:merchant",
            Self::Crafter => "minecraft:crafter_3x3",
        }
    }

    /// The engine's menu of this kind over its own slots: what the server
    /// runs, and what a client reads its slot positions and icons from.
    pub fn menu(self, own: Vec<Option<PlayerStack>>) -> Box<dyn Menu + Send> {
        match self {
            Self::Generic { rows } => Box::new(ChestMenu::new(usize::from(rows), own)),
            Self::Generic3x3 => Box::new(DispenserMenu::new(own)),
            Self::Hopper => Box::new(HopperMenu::new(own)),
            Self::ShulkerBox => Box::new(ShulkerBoxMenu::new(own)),
            Self::Furnace => Box::new(FurnaceMenu::new(CookingKind::Furnace, own)),
            Self::BlastFurnace => Box::new(FurnaceMenu::new(CookingKind::BlastFurnace, own)),
            Self::Smoker => Box::new(FurnaceMenu::new(CookingKind::Smoker, own)),
            Self::Merchant => Box::new(MerchantMenu::new(own)),
            Self::Crafter => Box::new(CrafterMenu::new(own)),
        }
    }
}

/// What the server reads of the player for a use or a menu batch: a copy of
/// its inventory (the carried stack is `Inventory.cursor`), taken when the
/// command is sent, and where it is.
#[derive(Clone, Debug, Default)]
pub struct PlayerContext {
    pub inventory: Inventory,
    /// `Inventory.selected`: the hotbar slot in hand.
    pub selected: usize,
    /// `getEyePosition`, for `stillValid`.
    pub eye: [f64; 3],
    /// The feet: where experience a menu awards appears.
    pub feet: [f64; 3],
    /// `hasInfiniteMaterials` (creative), which also gives the player a
    /// `block_interaction_range` of 5 instead of 4.5.
    pub creative: bool,
    /// `experienceLevel`.
    pub xp_level: i32,
    /// `enchantmentSeed`.
    pub enchantment_seed: i32,
}

/// The answer to a use that carried the player: whether the block opened
/// its menu (whose opening [`MenuUpdate`] is in the same output).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UseResult {
    pub pos: BlockPos,
    pub opened: bool,
    /// A message for the action bar (`sendOverlayMessage`): the container
    /// is locked (`container.isLocked` with its name).
    pub overlay: Option<Value>,
    /// Statistics the use counted (`awardStat`): (type, key, amount), as
    /// `Inventory::take_stat_events` gives them.
    pub stats: Vec<(String, String, i32)>,
}

/// The screen a menu opens (`ClientboundOpenScreenPacket`).
#[derive(Clone, Debug, PartialEq)]
pub struct MenuOpen {
    pub kind: MenuKind,
    /// The title, a text component as JSON: the container's custom name, or
    /// its translated default (`{"translate": "container.chest"}`).
    pub title: Value,
}

/// Kind-specific state for a screen. The storage menus have none; kinds
/// that do add a variant.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuExtra {
    /// The trader's offers, level and experience
    /// (`ClientboundMerchantOffersPacket`).
    Merchant(Offers),
}

impl MenuExtra {
    /// A client's copy of the menu takes the server's state (its own slots
    /// loaded first).
    pub fn sync(&self, menu: &mut (dyn Menu + Send)) {
        match self {
            Self::Merchant(offers) => {
                if let Some(menu) = merchant_mut(menu) {
                    menu.sync(offers.clone());
                }
            }
        }
    }
}

/// The trading menu behind a menu, if it is one.
pub fn merchant(menu: &(dyn Menu + Send)) -> Option<&MerchantMenu> {
    menu.as_any()?.downcast_ref()
}

/// The crafter's menu behind a menu, if it is one.
pub fn crafter(menu: &(dyn Menu + Send)) -> Option<&CrafterMenu> {
    menu.as_any()?.downcast_ref()
}

fn merchant_mut(menu: &mut (dyn Menu + Send)) -> Option<&mut MerchantMenu> {
    menu.as_any_mut()?.downcast_mut()
}

/// A menu's state for the client, after a batch, an opening, or a tick
/// that changed what it shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuUpdate {
    /// `containerId`, 1 to 100.
    pub id: u8,
    /// The `seq` of the last batch applied (0 before any).
    pub ack: u32,
    /// On the update that opens the menu: its screen.
    pub open: Option<MenuOpen>,
    /// The menu's own slots (the container's), all of them, in menu order.
    pub slots: Vec<Option<PlayerStack>>,
    /// The `ContainerData` values, in data-slot order (none for storage).
    pub data: Vec<i32>,
    pub extra: Option<MenuExtra>,
    /// The player's inventory slots the batch changed, against the copy it
    /// came with, with what they now hold. With `cursor`, present only on
    /// the answer to a batch (and on a `closed` update that answers a use).
    pub player: Vec<(usize, Option<PlayerStack>)>,
    /// The carried stack after the batch: `Some` exactly when this update
    /// carries the player's part.
    pub cursor: Option<Option<PlayerStack>>,
    /// Stacks to throw from the player (`Player.drop`), in order: dropped
    /// with the drop key or outside the window, or what did not fit back.
    pub thrown: Vec<PlayerStack>,
    /// Experience levels the batch spent.
    pub xp_levels: i32,
    /// The player's enchantment seed, when the batch re-rolled it
    /// (`Player.onEnchantmentPerformed`).
    pub enchantment_seed: Option<i32>,
    /// The player's ender inventory (27 slots) after the batch changed it,
    /// for the client to save.
    pub ender: Option<Vec<Option<PlayerStack>>>,
    /// Recipes the batch unlocked (`InventoryChangeTrigger`), in order.
    pub unlocked: Vec<String>,
    /// Statistics the batch counted.
    pub stats: Vec<(String, String, i32)>,
    /// `stillValid` failed: the container closed, and the screen should;
    /// the client answers with `Close`.
    pub closing: bool,
    /// The menu is gone (the answer to `Close`, or to a batch for a menu
    /// that is): the screen closes.
    pub closed: bool,
}

/// The server's menu state for its player.
pub(crate) struct Menus {
    open: Option<OpenMenu>,
    /// `ServerPlayer.containerCounter`.
    counter: u8,
    /// `Player.enderChestInventory`.
    ender: Vec<Option<PlayerStack>>,
    /// The ender inventory changed since the client heard of it.
    ender_changed: bool,
    /// `Player.random`, which menus draw from.
    random: LegacyRandom,
    /// The level events the menus made at their blocks since the client
    /// last heard of them: position, event id.
    level_events: Vec<(BlockPos, i32, i32)>,
}

impl Default for Menus {
    fn default() -> Self {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        Self { open: None, counter: 0, ender: vec![None; ENDER_SLOTS], ender_changed: false, random: LegacyRandom::new(seed), level_events: Vec::new() }
    }
}

/// An open menu.
struct OpenMenu {
    id: u8,
    kind: MenuKind,
    menu: Box<dyn Menu + Send>,
    source: Source,
    /// The last batch applied.
    seq: u32,
    /// The own slots and data as the client last heard of them.
    sent: (Vec<Option<PlayerStack>>, Vec<i32>),
    /// The kind's extra as the client last heard of it.
    sent_extra: Option<MenuExtra>,
    /// `stillValid` failed: the container is closed, and the client was
    /// asked to close its screen.
    closing: bool,
    /// Creative mode, for the reach.
    creative: bool,
}

/// What a menu's own slots are.
#[derive(Clone, Copy, Debug)]
enum Source {
    /// A block's container (both halves of a double chest).
    Container(ContainerRef),
    /// The player's ender inventory, through the ender chest at a position
    /// (`PlayerEnderChestContainer.activeChest`).
    EnderChest(LevelPos),
    /// The menu's own container, trading with this villager.
    Merchant(u64),
}

impl Source {
    /// The block entities the menu shows.
    fn positions(self) -> Vec<LevelPos> {
        match self {
            Self::Container(c) => c.positions(),
            Self::EnderChest(pos) => vec![pos],
            Self::Merchant(_) => Vec::new(),
        }
    }

    /// The menu's block, where its `ContainerLevelAccess` acts: the used
    /// block, the first half of a double chest. A trading screen has none.
    fn block(self) -> Option<LevelPos> {
        match self {
            Self::Container(ContainerRef::Single(pos, _) | ContainerRef::Double(pos, _)) | Self::EnderChest(pos) => Some(pos),
            Self::Merchant(_) => None,
        }
    }
}

/// The blocks whose use opens a menu, by block class (`useWithoutItem`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuBlock {
    Chest,
    EnderChest,
    Barrel,
    ShulkerBox,
    Hopper,
    Dropper,
    Dispenser,
    Furnace,
    BlastFurnace,
    Smoker,
    Crafter,
}

/// The menu blocks' classes, the most derived first. One line per kind.
const MENU_BLOCKS: &[(&str, MenuBlock)] = &[
    ("ChestBlock", MenuBlock::Chest),
    ("EnderChestBlock", MenuBlock::EnderChest),
    ("BarrelBlock", MenuBlock::Barrel),
    ("ShulkerBoxBlock", MenuBlock::ShulkerBox),
    ("HopperBlock", MenuBlock::Hopper),
    ("DropperBlock", MenuBlock::Dropper),
    ("DispenserBlock", MenuBlock::Dispenser),
    ("FurnaceBlock", MenuBlock::Furnace),
    ("BlastFurnaceBlock", MenuBlock::BlastFurnace),
    ("SmokerBlock", MenuBlock::Smoker),
    ("CrafterBlock", MenuBlock::Crafter),
];

fn menu_block(level: &Level<'_>, state: BlockStateId) -> Option<MenuBlock> {
    let blocks = &level.registries().blocks;
    let info = blocks.block(blocks.block_of(state));
    MENU_BLOCKS.iter().find(|(class, _)| info.is_a(class)).map(|&(_, block)| block)
}

/// What a use opens: the menu, its storage and title, the statistic the use
/// counts, and the block entities whose locks it must pass.
struct Target {
    kind: MenuKind,
    source: Source,
    title: Value,
    /// The statistic the use counts (`awardStat`); empty for a block that
    /// counts none (the crafter).
    stat: &'static str,
    locked_by: Vec<LevelPos>,
    /// Where the locked sound plays.
    centre: [f64; 3],
}

/// A translated title.
fn translated(key: &str) -> Value {
    json!({ "translate": key })
}

fn centre(pos: LevelPos) -> [f64; 3] {
    [f64::from(pos.x) + 0.5, f64::from(pos.y) + 0.5, f64::from(pos.z) + 0.5]
}

/// `Player.blockInteractionRange`: the attribute's 4.5, and 0.5 more in
/// creative.
fn interaction_range(creative: bool) -> f64 {
    if creative { 5.0 } else { 4.5 }
}

/// `Player.isWithinBlockInteractionRange(pos, 4.0)`: the eye is nearer the
/// block's cube than the reach and 4.
fn within_reach(pos: LevelPos, eye: [f64; 3], creative: bool) -> bool {
    let low = [f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)];
    let distance: f64 = (0..3).map(|a| (low[a] - eye[a]).max(eye[a] - (low[a] + 1.0)).max(0.0).powi(2)).sum();
    let range = interaction_range(creative) + 4.0;
    distance < range * range
}

/// The player's height for its eye height (standing, crouching, or
/// swimming and gliding).
fn player_height(eye_height: f32) -> f32 {
    if eye_height >= 1.5 {
        1.8
    } else if eye_height >= 1.0 {
        1.5
    } else {
        0.6
    }
}

/// `LockCode.canUnlock`: the stack in hand meets the lock's item predicate
/// (`ItemPredicate.test`): its items, count and exact components. A lock
/// with sub-predicates, or naming an item tag, is not read, and stays shut.
fn unlocks(lock: &Tag, held: Option<&PlayerStack>) -> bool {
    let (id, count) = held.map_or(("minecraft:air", 0), |stack| (stack.id.as_str(), i64::from(stack.count)));
    let id_matches = |item: &Tag| item.as_str().is_some_and(|name| !name.starts_with('#') && (name == id || format!("minecraft:{name}") == id));
    let items = match lock.get("items") {
        None => true,
        Some(Tag::List(list)) => list.iter().any(id_matches),
        Some(item) => id_matches(item),
    };
    let count_matches = match lock.get("count") {
        None => true,
        Some(Tag::Compound(range)) => range.get("min").and_then(Tag::as_i64).is_none_or(|min| count >= min) && range.get("max").and_then(Tag::as_i64).is_none_or(|max| count <= max),
        Some(exact) => exact.as_i64() == Some(count),
    };
    let held_components = held.and_then(|stack| stack.components.as_ref()).and_then(stacks::components_tag);
    let components = match lock.get("components") {
        None => true,
        Some(Tag::Compound(expected)) => expected.iter().all(|(key, value)| held_components.as_ref().and_then(|c| c.get(key)) == Some(value)),
        Some(_) => false,
    };
    let predicates = lock.get("predicates").is_none_or(|p| p.as_compound().is_some_and(|map| map.is_empty()));
    items && count_matches && components && predicates
}

/// The player's inventory as a batch's copy had it, for the answer's
/// player part.
struct Snapshot {
    slots: Vec<Option<PlayerStack>>,
    unlocked: usize,
}

impl Snapshot {
    /// The copy before the batch, its statistics not yet taken set aside:
    /// the client counts those itself.
    fn of(inventory: &mut Inventory) -> Self {
        let _ = inventory.take_stat_events();
        Self { slots: inventory.slots.clone(), unlocked: inventory.unlock_sequence().len() }
    }

    /// What the batch did to the player.
    fn answer(&self, inventory: &mut Inventory, update: &mut MenuUpdate) {
        update.player = inventory.slots.iter().zip(&self.slots).enumerate().filter(|(_, (now, was))| now != was).map(|(slot, (now, _))| (slot, now.clone())).collect();
        update.cursor = Some(inventory.cursor.clone());
        update.unlocked = inventory.unlock_sequence().iter().skip(self.unlocked).cloned().collect();
        update.stats.extend(inventory.take_stat_events());
    }
}

/// The recipe book and its item catalog as the level's furnaces read them:
/// the server's `RecipeManager`, and the items' `cooking_fuel` components.
pub(crate) struct BookCooking(pub std::sync::Arc<RecipeBook>);

impl BookCooking {
    fn kind(kind: CookingType) -> CookingKind {
        match kind {
            CookingType::Smelting => CookingKind::Furnace,
            CookingType::Blasting => CookingKind::BlastFurnace,
            CookingType::Smoking => CookingKind::Smoker,
        }
    }

    fn recipe_of(id: &str, recipe: &minecraftoss_player::crafting::SmeltingRecipe) -> CookingRecipe {
        CookingRecipe { id: id.to_owned(), result: stacks::to_level(&recipe.result), cooking_time: recipe.cooking_ticks as i32, experience: recipe.experience }
    }
}

impl Cooking for BookCooking {
    fn recipe_for(&self, kind: CookingType, ingredient: &LevelStack) -> Option<CookingRecipe> {
        let input = PlayerStack::new(ingredient.id.clone(), 1);
        let (id, recipe) = self.0.cooking_recipe_for(Self::kind(kind), &input)?;
        Some(Self::recipe_of(id, recipe))
    }

    fn recipe(&self, id: &str) -> Option<CookingRecipe> {
        self.0.cooking_recipe(id).map(|recipe| Self::recipe_of(id, recipe))
    }

    fn is_fuel(&self, id: &str) -> bool {
        self.0.is_fuel(id)
    }

    fn burn_time(&self, id: &str) -> i32 {
        self.0.fuel_ticks(id) as i32
    }

    fn speed_multiplier(&self, id: &str) -> f32 {
        self.0.fuel_speed(id)
    }

    fn remainder(&self, id: &str) -> Option<LevelStack> {
        self.0.crafting_remainder(id).as_ref().map(stacks::to_level)
    }

    fn bottom_takeable(&self, id: &str) -> bool {
        self.0.item_in_tag("minecraft:furnace_fuel_bottom_takeable", id)
    }
}

impl Crafting for BookCooking {
    /// `RecipeBook::matching` over the grid, and each stack's crafting
    /// remainder (`CraftingRecipe.defaultCraftingReminder`).
    fn craft(&self, grid: &[LevelStack]) -> Option<(LevelStack, Vec<LevelStack>)> {
        let max = |id: &str| i32::from(self.0.max_stack(id));
        let grid: Vec<Option<PlayerStack>> = grid.iter().map(|stack| stacks::to_player(stack, max)).collect();
        let result = self.0.matching(&grid, 3, 3)?;
        let remainders = grid.iter().flatten().filter_map(|stack| self.0.crafting_remainder(&stack.id)).map(|stack| stacks::to_level(&stack)).collect();
        Some((stacks::to_level(&result), remainders))
    }
}

/// A menu context for the player.
fn context<'a>(inventory: &'a mut Inventory, random: &'a mut LegacyRandom, player: &PlayerContext) -> MenuContext<'a> {
    let mut cx = MenuContext::new(inventory, random);
    cx.selected = player.selected;
    cx.creative = player.creative;
    cx.xp_level = player.xp_level;
    cx.enchantment_seed = player.enchantment_seed;
    cx
}

impl ServerSim {
    /// The player's ender inventory as saved (`EnderItems`), at world load.
    pub fn set_ender_items(&mut self, mut items: Vec<Option<PlayerStack>>) {
        items.resize(ENDER_SLOTS, None);
        self.menus.ender = items;
        self.menus.ender_changed = false;
    }

    /// The player's ender inventory.
    pub fn ender_items(&self) -> &[Option<PlayerStack>] {
        &self.menus.ender
    }

    /// The menu open now: its id and kind.
    pub fn open_menu(&self) -> Option<(u8, MenuKind)> {
        self.menus.open.as_ref().map(|open| (open.id, open.kind))
    }

    /// Whether the menu open now trades with this villager.
    pub(crate) fn trades_with(&self, villager: u64) -> bool {
        self.menus.open.as_ref().is_some_and(|open| matches!(open.source, Source::Merchant(v) if v == villager))
    }

    /// Whether using a block in this state opens a menu (the client then
    /// sends the use with the player, and swings).
    pub fn opens_menu(&self, state: BlockStateId) -> bool {
        menu_block(&self.level, state).is_some()
    }

    /// A use of the block at `pos` by the player (`useWithoutItem`): a menu
    /// block opens its menu if its rules allow (`openMenu` closing the one
    /// open first); another block is used as without the player. Returns
    /// the answer and the menu updates it made (the old menu's closing,
    /// the new one's opening).
    pub fn use_block_with(&mut self, pos: BlockPos, facing: &str, mut player: PlayerContext) -> (UseResult, Vec<MenuUpdate>) {
        let at = LevelPos::new(pos.0, pos.1, pos.2);
        let mut result = UseResult { pos, ..UseResult::default() };
        let Some(block) = menu_block(&self.level, self.level.block(at)) else {
            self.use_block(pos, facing);
            return (result, Vec::new());
        };
        self.update_sitting_cats();
        let Some(target) = self.menu_target(block, at) else { return (result, Vec::new()) };
        // `openMenu` runs, and the block counts the use.
        if !target.stat.is_empty() {
            result.stats.push((minecraftoss_player::statistics::CUSTOM.to_owned(), format!("minecraft:{}", target.stat), 1));
        }
        let mut inventory = std::mem::take(&mut player.inventory);
        let mut updates: Vec<MenuUpdate> = self.close_for_open(&mut inventory, &player).into_iter().collect();
        // `BaseContainerBlockEntity.createMenu`: a lock the hand does not
        // open refuses, with a message and a click.
        let held = inventory.slots.get(player.selected).and_then(Option::as_ref);
        let shut = target.locked_by.iter().any(|&p| self.level.block_entity(p).and_then(|t| t.get("lock")).is_some_and(|lock| !unlocks(lock, held)));
        if shut {
            result.overlay = Some(json!({ "translate": "container.isLocked", "with": [target.title] }));
            self.level.play_sound("minecraft:block.chest.locked", target.centre, 1.0, 1.0);
            return (result, updates);
        }
        // `RandomizableContainerBlockEntity.createMenu`, and the double
        // chest's provider for each half in turn: the loot table fills the
        // container for the player first.
        if let Source::Container(c) = target.source {
            for p in c.positions() {
                self.level.unpack_loot_table(p, Some(0.0));
            }
        }
        // `nextContainerCounter`.
        self.menus.counter = self.menus.counter % 100 + 1;
        let own = self.source_items(target.source);
        let mut menu = target.kind.menu(own);
        self.load_data(menu.as_mut(), target.source);
        let sent = (menu.own().items().to_vec(), menu.data());
        let open = OpenMenu { id: self.menus.counter, kind: target.kind, menu, source: target.source, seq: 0, sent: sent.clone(), sent_extra: None, closing: false, creative: player.creative };
        // The menu's `startOpen`.
        for p in target.source.positions() {
            self.level.start_open(p, interaction_range(player.creative));
            self.level.watch_block_entity(p);
        }
        updates.push(MenuUpdate { id: open.id, open: Some(MenuOpen { kind: target.kind, title: target.title }), slots: sent.0, data: sent.1, ..MenuUpdate::default() });
        self.menus.open = Some(open);
        result.opened = true;
        (result, updates)
    }

    /// `openMenu` closes the menu open first (`closeContainer`), against the
    /// copy of the inventory the opening came with: the update that closes
    /// it, with its player part.
    fn close_for_open(&mut self, inventory: &mut Inventory, player: &PlayerContext) -> Option<MenuUpdate> {
        let open = self.menus.open.take()?;
        let before = Snapshot::of(inventory);
        let mut random = self.menus.random.clone();
        let mut cx = context(inventory, &mut random, player);
        let mut update = MenuUpdate { id: open.id, ack: open.seq, closed: true, ..MenuUpdate::default() };
        let block = open.source.block();
        self.finish_close(open, &mut cx);
        update.thrown = std::mem::take(&mut cx.thrown);
        self.menu_effects(&mut cx, block, player.feet);
        drop(cx);
        self.menus.random = random;
        before.answer(inventory, &mut update);
        Some(update)
    }

    /// `Merchant.openTradingScreen` for a villager the player now trades
    /// with (`Villager.startTrading`), titled with its name (its
    /// profession's): the menu open before closes, and the screen opens with
    /// the villager's offers. Returns the menu updates it made.
    pub(crate) fn open_merchant(&mut self, villager: u64, inventory: &mut Inventory, player: &PlayerContext) -> Vec<MenuUpdate> {
        let mut updates: Vec<MenuUpdate> = self.close_for_open(inventory, player).into_iter().collect();
        let (Some(offers), Some(entity)) = (self.mobs.merchant_offers(villager), self.mobs.villagers().iter().find(|e| e.id == villager)) else { return updates };
        let profession = entity.villager.profession.id();
        let (namespace, path) = profession.split_once(':').unwrap_or(("minecraft", profession));
        let title = translated(&format!("entity.{namespace}.villager.{path}"));
        self.menus.counter = self.menus.counter % 100 + 1;
        let mut menu = MerchantMenu::new(Vec::new());
        menu.set_offers(offers);
        let sent = (menu.own().items().to_vec(), menu.data());
        let extra = Some(MenuExtra::Merchant(menu.offers().clone()));
        let kind = MenuKind::Merchant;
        let open = OpenMenu { id: self.menus.counter, kind, menu: Box::new(menu), source: Source::Merchant(villager), seq: 0, sent: sent.clone(), sent_extra: extra.clone(), closing: false, creative: player.creative };
        updates.push(MenuUpdate { id: open.id, open: Some(MenuOpen { kind, title }), slots: sent.0, data: sent.1, extra, ..MenuUpdate::default() });
        self.menus.open = Some(open);
        updates
    }

    /// A trading screen and its villager: what the inputs told the villager
    /// is told it (`notifyTradeUpdated`, `notifyTrade`), with the statistic
    /// a trade counts (`TRADED_WITH_VILLAGER`) and the orbs it dropped; then
    /// the menu takes the villager's offers as they now are. When they
    /// changed otherwise (a new level's offers or a restock, through
    /// `updateSpecialPrices`), the result is read for them
    /// (`updateSellItem`).
    fn merchant_sync(&mut self, open: &mut OpenMenu, stats: &mut Vec<(String, String, i32)>) {
        let Source::Merchant(villager) = open.source else { return };
        let Some(menu) = merchant_mut(open.menu.as_mut()) else { return };
        let events = menu.take_events();
        let trades = events.iter().filter(|e| matches!(e, minecraftoss_entities::merchant::MerchantEvent::Trade(_))).count();
        stats.extend((0..trades).map(|_| (minecraftoss_player::statistics::CUSTOM.to_owned(), "minecraft:traded_with_villager".to_owned(), 1)));
        self.mobs.merchant_notify(villager, &events);
        self.spawn_trade_experience();
        let Some(offers) = self.mobs.merchant_offers(villager) else { return };
        if *menu.offers() != offers {
            menu.set_offers(offers);
            menu.update_sell_item();
            self.mobs.merchant_notify(villager, &menu.take_events());
        }
    }

    /// The kind's extra for the screen, as the menu has it now.
    fn extra(open: &OpenMenu) -> Option<MenuExtra> {
        merchant(open.menu.as_ref()).map(|menu| MenuExtra::Merchant(menu.offers().clone()))
    }

    /// The menu a block opens, by its `useWithoutItem` and
    /// `getMenuProvider`; none when it refuses to open.
    fn menu_target(&self, block: MenuBlock, at: LevelPos) -> Option<Target> {
        let level = &self.level;
        let custom_name = |p: LevelPos| level.block_entity(p).and_then(|t| t.get("CustomName")).map(stacks::text_json);
        let single = |kind: MenuKind, key: &str, stat: &'static str| -> Option<Target> {
            let c = level.container_at(at, true)?;
            Some(Target { kind, source: Source::Container(c), title: custom_name(at).unwrap_or_else(|| translated(key)), stat, locked_by: vec![at], centre: centre(at) })
        };
        match block {
            MenuBlock::Chest => {
                // `ChestBlock.getMenuProvider`: none when it or its other
                // half is blocked.
                let c = level.container_at(at, false)?;
                let trapped = level.registries().blocks.block(level.registries().blocks.block_of(level.block(at))).is_a("TrappedChestBlock");
                let stat = if trapped { "trigger_trapped_chest" } else { "open_chest" };
                match c {
                    ContainerRef::Single(..) => single(MenuKind::Generic { rows: 3 }, "container.chest", stat),
                    ContainerRef::Double(first, second) => {
                        let title = custom_name(first).or_else(|| custom_name(second)).unwrap_or_else(|| translated("container.chestDouble"));
                        let (a, b) = (centre(first), centre(second));
                        let middle = [(a[0] + b[0]) / 2.0, a[1], (a[2] + b[2]) / 2.0];
                        Some(Target { kind: MenuKind::Generic { rows: 6 }, source: Source::Container(c), title, stat, locked_by: vec![first, second], centre: middle })
                    }
                }
            }
            MenuBlock::EnderChest => {
                // `EnderChestBlock.useWithoutItem`: only the block above
                // can stop it.
                level.block_entity(at)?;
                let above = level.block(at.above());
                if level.registries().blocks.is(above, minecraftoss_core::block::flags::REDSTONE_CONDUCTOR) {
                    return None;
                }
                Some(Target { kind: MenuKind::Generic { rows: 3 }, source: Source::EnderChest(at), title: translated("container.enderchest"), stat: "open_enderchest", locked_by: Vec::new(), centre: centre(at) })
            }
            MenuBlock::Barrel => single(MenuKind::Generic { rows: 3 }, "container.barrel", "open_barrel"),
            MenuBlock::ShulkerBox => {
                if level.block_entity(at).is_none() || !level.shulker_box_can_open(at) {
                    return None;
                }
                single(MenuKind::ShulkerBox, "container.shulkerBox", "open_shulker_box")
            }
            MenuBlock::Hopper => single(MenuKind::Hopper, "container.hopper", "inspect_hopper"),
            MenuBlock::Dropper => single(MenuKind::Generic3x3, "container.dropper", "inspect_dropper"),
            MenuBlock::Dispenser => single(MenuKind::Generic3x3, "container.dispenser", "inspect_dispenser"),
            MenuBlock::Furnace => single(MenuKind::Furnace, "container.furnace", "interact_with_furnace"),
            MenuBlock::BlastFurnace => single(MenuKind::BlastFurnace, "container.blast_furnace", "interact_with_blast_furnace"),
            MenuBlock::Smoker => single(MenuKind::Smoker, "container.smoker", "interact_with_smoker"),
            MenuBlock::Crafter => single(MenuKind::Crafter, "container.crafter", ""),
        }
    }

    /// A batch of inputs on the menu `id` with a copy of the player's
    /// inventory (`ServerGamePacketListenerImpl`'s menu handlers): each
    /// applied in order while the menu is `stillValid` (vanilla ignores
    /// them otherwise), up to a `Close`. The answer has the menu's state and
    /// the player's part. A batch for a menu that is gone, or that the
    /// server asked to close, only returns the carried stack, and closes it.
    pub fn menu_batch(&mut self, id: u8, seq: u32, inputs: &[MenuInput], mut player: PlayerContext) -> MenuUpdate {
        let mut inventory = std::mem::take(&mut player.inventory);
        let before = Snapshot::of(&mut inventory);
        let mut random = self.menus.random.clone();
        let mut cx = context(&mut inventory, &mut random, &player);
        let mut update = MenuUpdate { id, ack: seq, ..MenuUpdate::default() };
        let block = self.menus.open.as_ref().filter(|open| open.id == id).and_then(|open| open.source.block());
        match self.menus.open.take() {
            Some(mut open) if open.id == id && !open.closing => {
                open.creative = player.creative;
                open.seq = seq;
                self.reload(&mut open);
                self.merchant_sync(&mut open, &mut update.stats);
                let valid = self.still_valid(&open, player.eye);
                let closing = inputs.contains(&MenuInput::Close);
                for input in inputs.iter().take_while(|input| **input != MenuInput::Close) {
                    if valid {
                        menu::handle(open.menu.as_mut(), &mut cx, input);
                    }
                }
                self.write_back(&mut open, &cx.block_requests);
                self.answer_requests(&open, &mut cx, player.feet);
                self.merchant_sync(&mut open, &mut update.stats);
                if closing {
                    self.finish_close(open, &mut cx);
                    update.closed = true;
                } else {
                    self.reload(&mut open);
                    update.slots = open.menu.own().items().to_vec();
                    update.data = open.menu.data();
                    update.extra = Self::extra(&open);
                    open.sent = (update.slots.clone(), update.data.clone());
                    open.sent_extra = update.extra.clone();
                    self.menus.open = Some(open);
                }
            }
            Some(open) if open.id == id => {
                self.finish_close(open, &mut cx);
                update.closed = true;
            }
            other => {
                self.menus.open = other;
                menu::return_carried(&mut cx);
                update.closed = true;
            }
        }
        update.thrown = std::mem::take(&mut cx.thrown);
        update.xp_levels = cx.xp_levels_spent;
        update.enchantment_seed = (cx.enchantment_seed != player.enchantment_seed).then_some(cx.enchantment_seed);
        self.menu_effects(&mut cx, block, player.feet);
        drop(cx);
        self.menus.random = random;
        before.answer(&mut inventory, &mut update);
        if self.menus.ender_changed {
            self.menus.ender_changed = false;
            update.ender = Some(self.menus.ender.clone());
        }
        update
    }

    /// What the inputs did in the world: experience orbs at the player's
    /// feet or the centre of the menu's block, and the level events and
    /// sounds at the block (`ContainerLevelAccess.execute`).
    fn menu_effects(&mut self, cx: &mut MenuContext, block: Option<LevelPos>, feet: [f64; 3]) {
        for (place, amount) in std::mem::take(&mut cx.xp_orbs) {
            let at = match place {
                MenuPlace::Player => Some(feet),
                MenuPlace::Block => block.map(centre),
            };
            if let Some(at) = at {
                self.level.award_experience(at, amount);
            }
        }
        let events = std::mem::take(&mut cx.level_events);
        let sounds = std::mem::take(&mut cx.sounds);
        let Some(pos) = block else { return };
        self.menus.level_events.extend(events.into_iter().map(|id| ((pos.x, pos.y, pos.z), id, 0)));
        for (event, volume, pitch) in sounds {
            self.level.play_sound(event, centre(pos), volume, pitch);
        }
    }

    /// The level events the menus and then the level made since the last
    /// call (position, event id, data), for the client's
    /// `LevelEventHandler`.
    pub fn take_level_events(&mut self) -> Vec<(BlockPos, i32, i32)> {
        let mut events = std::mem::take(&mut self.menus.level_events);
        events.extend(self.level.take_level_events().into_iter().map(|(pos, id, data)| ((pos.x, pos.y, pos.z), id, data)));
        events
    }

    /// `doCloseContainer`: the menu's `removed` (the carried stack goes
    /// back, or is thrown), then the container's `stopOpen`, unless
    /// `stillValid` closed it already.
    fn finish_close(&mut self, mut open: OpenMenu, cx: &mut MenuContext) {
        menu::handle(open.menu.as_mut(), cx, &MenuInput::Close);
        self.write_back(&mut open, &cx.block_requests);
        cx.block_requests.clear();
        if !open.closing {
            self.stop_open(&open);
        }
        for p in open.source.positions() {
            self.level.unwatch_block_entity(p);
        }
    }

    /// The container's `stopOpen`, each half of a double chest in turn; a
    /// trading screen's villager stops trading (`setTradingPlayer(null)`).
    fn stop_open(&mut self, open: &OpenMenu) {
        if let Source::Merchant(villager) = open.source {
            self.mobs.merchant_stop(villager);
        }
        for p in open.source.positions() {
            self.level.stop_open(p);
        }
    }

    /// The own slots from the menu's storage; a crafter's with what its
    /// grid crafts (`CrafterMenu.refreshRecipeResult`).
    fn source_items(&self, source: Source) -> Vec<Option<PlayerStack>> {
        match source {
            Source::Container(c) => {
                let items = &self.level.registries().items;
                let mut own: Vec<Option<PlayerStack>> = self.level.container_items(c).iter().map(|stack| stacks::to_player(stack, |id| items.max_stack(id))).collect();
                if let ContainerRef::Single(pos, Store::Crafter) = c {
                    own.push(self.level.crafter_result(pos).and_then(|stack| stacks::to_player(&stack, |id| items.max_stack(id))));
                }
                own
            }
            Source::EnderChest(_) => self.menus.ender.clone(),
            Source::Merchant(_) => Vec::new(),
        }
    }

    /// The menu reads its storage again (what hoppers and dispensers did),
    /// and its data values.
    fn reload(&self, open: &mut OpenMenu) {
        if let Source::Merchant(_) = open.source {
            return;
        }
        let items = self.source_items(open.source);
        open.menu.own_mut().load(items);
        self.load_data(open.menu.as_mut(), open.source);
    }

    /// The data values from the menu's block entity (a furnace's progress,
    /// a crafter's slot states).
    fn load_data(&self, menu: &mut (dyn Menu + Send), source: Source) {
        let data = match source {
            Source::Container(ContainerRef::Single(pos, Store::Furnace)) => self.level.furnace_data(pos).map(|data| data.to_vec()),
            Source::Container(ContainerRef::Single(pos, Store::Crafter)) => self.level.crafter_data(pos).map(|data| data.to_vec()),
            _ => None,
        };
        for (id, value) in data.into_iter().flatten().enumerate() {
            menu.set_data(id, value);
        }
    }

    /// What the inputs asked of the menu's block entity, after their slots
    /// went back: a furnace pays the experience of the recipes it used at
    /// the player's feet, and the player unlocks them (`awardRecipes`),
    /// once for every take; a crafter's slots take their states, in order.
    fn answer_requests(&mut self, open: &OpenMenu, cx: &mut MenuContext, feet: [f64; 3]) {
        let requests = std::mem::take(&mut cx.block_requests);
        let Source::Container(ContainerRef::Single(pos, store)) = open.source else { return };
        for request in requests {
            match (store, request) {
                (Store::Furnace, BlockRequest::AwardUsedRecipes) => {
                    for id in self.level.award_used_recipes(pos, feet) {
                        cx.inventory.unlock_recipe(&id);
                    }
                }
                (Store::Crafter, BlockRequest::SlotState { slot, enabled }) => self.level.crafter_set_slot_state(pos, slot, enabled),
                _ => {}
            }
        }
    }

    /// The own slots the inputs changed go back to the storage: a block's
    /// through `Container.setItem`, then `setChanged` (comparators), or as
    /// `removeItem` left them where a take emptied them.
    fn write_back(&mut self, open: &mut OpenMenu, requests: &[BlockRequest]) {
        let changed = open.menu.own_mut().take_changed();
        if changed.is_empty() {
            return;
        }
        let own = open.menu.own().items();
        match open.source {
            Source::Container(c) => {
                for slot in changed.into_iter().filter(|&slot| slot < c.size()) {
                    let stack = own[slot].as_ref().map_or_else(LevelStack::empty, stacks::to_level);
                    if stack.is_empty() && requests.contains(&BlockRequest::Emptied(slot)) {
                        self.level.container_set_taken(c, slot, stack);
                    } else {
                        self.level.container_set_item(c, slot, stack);
                    }
                }
                self.level.container_set_changed(c);
            }
            Source::EnderChest(_) => {
                for slot in changed.into_iter().filter(|&slot| slot < ENDER_SLOTS) {
                    self.menus.ender[slot] = own[slot].clone();
                }
                self.menus.ender_changed = true;
            }
            Source::Merchant(_) => {}
        }
    }

    /// `AbstractContainerMenu.stillValid`: every block entity the menu
    /// shows is the one it opened (`Container.stillValidBlockEntity`) and
    /// within reach of the eye; for the ender inventory, its active chest.
    fn still_valid(&self, open: &OpenMenu, eye: [f64; 3]) -> bool {
        if let Source::Merchant(villager) = open.source {
            return self.mobs.merchant_still_valid(villager, 0, glam::DVec3::from_array(eye), open.creative);
        }
        open.source.positions().into_iter().all(|p| self.level.block_entity_still_there(p) && within_reach(p, eye, open.creative))
    }

    /// The cats sitting in their pose (`Cat.isInSittingPose`, the saved
    /// `Sitting`), which block chests: dormant ones, as cats are not
    /// simulated.
    fn update_sitting_cats(&mut self) {
        self.level.sitting_cats = self
            .dormant
            .iter()
            .filter(|(group, tag)| group[0].kind == "minecraft:cat" && tag.get("Sitting").and_then(Tag::as_i64) == Some(1))
            .map(|(group, _)| {
                let b = group[0].bb;
                Aabb::new(b[0], b[1], b[2], b[3], b[4], b[5])
            })
            .collect();
    }

    /// Before a tick: the openers' rechecks see the player whose menu shows
    /// their container (`ContainerUser`), and chests see the cats on them.
    pub(crate) fn prepare_menus(&mut self, players: &[PlayerCandidate]) {
        self.update_sitting_cats();
        self.level.container_users.clear();
        let Some(open) = self.menus.open.as_ref().filter(|open| !open.closing) else { return };
        let Some(player) = players.iter().find(|p| p.id == 0 && !p.spectator) else { return };
        let p = player.position;
        let bounding_box = Aabb::for_entity(p.x, p.y, p.z, 0.6, player_height(player.eye_height));
        self.level.container_users.push(ContainerUser { bounding_box, range: interaction_range(open.creative), open: open.source.positions() });
    }

    /// `ServerPlayer.tick` for the open menu, after the level and the mobs:
    /// `broadcastChanges` sends the own slots and data again when they
    /// changed; then `stillValid`, which on failing closes the container and
    /// asks the client to close the screen.
    pub(crate) fn menu_tick(&mut self, players: &[PlayerCandidate]) -> Option<MenuUpdate> {
        let mut open = self.menus.open.take()?;
        let update = self.menu_tick_open(&mut open, players);
        self.menus.open = Some(open);
        update
    }

    fn menu_tick_open(&mut self, open: &mut OpenMenu, players: &[PlayerCandidate]) -> Option<MenuUpdate> {
        if open.closing {
            return None;
        }
        self.reload(open);
        let mut stats = Vec::new();
        self.merchant_sync(open, &mut stats);
        let shown = (open.menu.own().items().to_vec(), open.menu.data());
        let extra = Self::extra(open);
        let changed = shown != open.sent || extra != open.sent_extra;
        let eye = players.iter().find(|p| p.id == 0).map(|p| (p.position + glam::DVec3::Y * f64::from(p.eye_height)).to_array());
        let valid = eye.is_none_or(|eye| self.still_valid(open, eye));
        if !valid {
            self.stop_open(open);
            open.closing = true;
        }
        if !changed && valid {
            return None;
        }
        open.sent = shown.clone();
        open.sent_extra = extra.clone();
        Some(MenuUpdate { id: open.id, ack: open.seq, slots: shown.0, data: shown.1, extra, closing: !valid, ..MenuUpdate::default() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Block, HandcraftedScene};
    use crate::server::{Breaker, EditBy, PlayerEdit};
    use crate::terrain::BlockStates;
    use minecraftoss_core::pos::Direction;
    use minecraftoss_core::registries::DataPaths;
    use minecraftoss_core::{ChunkPos, Registries};
    use minecraftoss_generator::terrain::TerrainGenerator;
    use minecraftoss_world::chunk_map::{ChunkMap, WorldGen};
    use minecraftoss_world::level::openers::LidStatus;
    use std::sync::Arc;

    /// A server over the nine chunks around the origin, with the scene the
    /// client shows; none without the game data.
    fn world() -> Option<(ServerSim, HandcraftedScene)> {
        let paths = DataPaths::discover().ok()?;
        let registries = Arc::new(Registries::load(&paths).ok()?);
        registries.block_entities.as_ref()?;
        let worldgen = Arc::new(WorldGen::new(Arc::new(TerrainGenerator::overworld(registries.clone(), 0).ok()?)).ok()?);
        let states = Arc::new(BlockStates::new(registries.clone(), 0, -64, 384).ok()?);
        let mut map = ChunkMap::with_worldgen(worldgen.clone(), 2, 4);
        let mut server = ServerSim::new(worldgen, states.clone(), "minecraft:overworld");
        let mut scene = HandcraftedScene::streamed(states);
        for x in -1..=1 {
            for z in -1..=1 {
                let chunk = map.load_now(ChunkPos::new(x, z));
                server.load_chunk(&chunk);
                scene.insert_chunk(chunk);
            }
        }
        server.take_changes();
        server.take_block_entity_changes();
        Some((server, scene))
    }

    fn place(server: &mut ServerSim, scene: &mut HandcraftedScene, pos: BlockPos, block: Block) {
        scene.set(pos, Some(block.clone()));
        server.player_edit_block(pos, Some(&block), PlayerEdit::Place);
    }

    fn stack(id: &str, count: u8, max: u8) -> PlayerStack {
        PlayerStack { id: id.to_owned(), count, max, components: None }
    }

    /// The player standing at `feet`, as the tick sees it.
    fn candidate(feet: [f64; 3]) -> PlayerCandidate {
        PlayerCandidate {
            id: 0,
            position: glam::DVec3::from_array(feet),
            eye_height: 1.62,
            main_hand_cow_food: false,
            offhand_cow_food: false,
            main_hand_pig_food: false,
            offhand_pig_food: false,
            main_hand_chicken_food: false,
            offhand_chicken_food: false,
            main_hand_carrot_on_a_stick: false,
            offhand_carrot_on_a_stick: false,
            main_hand_wolf_interest: false,
            offhand_wolf_interest: false,
            main_hand_horse_tempt: false,
            offhand_horse_tempt: false,
            alive: true,
            spectator: false,
            attackable: true,
        }
    }

    /// The player two blocks south of the origin chest, with `inventory`.
    fn player(inventory: &Inventory) -> PlayerContext {
        PlayerContext { inventory: inventory.clone(), selected: 0, eye: [8.5, 201.62, 10.5], feet: [8.5, 200.0, 10.5], ..PlayerContext::default() }
    }

    /// What the client does with an answer's player part.
    fn apply(inventory: &mut Inventory, update: &MenuUpdate) {
        if let Some(cursor) = &update.cursor {
            for (slot, stack) in &update.player {
                inventory.slots[*slot] = stack.clone();
            }
            inventory.cursor = cursor.clone();
        }
    }

    /// A server tick as the loop runs it, for the menu.
    fn tick(server: &mut ServerSim, feet: [f64; 3]) -> Option<MenuUpdate> {
        let players = [candidate(feet)];
        server.set_players(&[feet], 2);
        server.prepare_menus(&players);
        server.tick();
        server.menu_tick(&players)
    }

    fn click(slot: i32, kind: ContainerInput) -> MenuInput {
        MenuInput::Click { slot, button: 0, kind }
    }

    fn items(server: &ServerSim, pos: BlockPos) -> Vec<Option<PlayerStack>> {
        let level_items = server.level.block_container_items(LevelPos::new(pos.0, pos.1, pos.2)).expect("a container");
        let catalog = &server.level.registries().items;
        level_items.iter().map(|s| stacks::to_player(s, |id| catalog.max_stack(id))).collect()
    }

    const FEET: [f64; 3] = [8.5, 200.0, 10.5];

    /// A chest opens with its sound and lid, takes and gives items by
    /// clicks and shift-clicks, writes them to its block entity, answers
    /// each batch with the player's changes, and closes.
    #[test]
    fn a_chest_menu_moves_items_both_ways() {
        let Some((mut server, mut scene)) = world() else { return };
        let pos = (8, 200, 8);
        place(&mut server, &mut scene, pos, Block::new("minecraft:chest"));
        server.level.take_sounds();
        let mut inventory = Inventory::default();
        inventory.slots[0] = Some(stack("minecraft:diamond", 10, 64));
        inventory.slots[9] = Some(stack("minecraft:oak_log", 5, 64));

        let (result, updates) = server.use_block_with(pos, "north", player(&inventory));
        assert!(result.opened);
        assert_eq!(result.stats, vec![("minecraft:custom".to_owned(), "minecraft:open_chest".to_owned(), 1)]);
        let [opening] = &updates[..] else { panic!("one opening: {updates:?}") };
        assert_eq!((opening.id, opening.ack, opening.cursor.as_ref()), (1, 0, None));
        assert_eq!(opening.open, Some(MenuOpen { kind: MenuKind::Generic { rows: 3 }, title: json!({"translate": "container.chest"}) }));
        assert_eq!(opening.slots, vec![None; 27]);
        let sounds = server.level.take_sounds();
        assert_eq!(sounds.len(), 1, "{sounds:?}");
        assert_eq!((sounds[0].event, sounds[0].position, sounds[0].volume), ("minecraft:block.chest.open", [8.5, 200.5, 8.5], 0.5));
        assert!((0.9..1.0).contains(&sounds[0].pitch));
        // The lid opens with the next tick's block events.
        assert_eq!(tick(&mut server, FEET), None, "nothing changed");
        assert_eq!(server.take_block_events(), vec![crate::server::BlockEventView { pos, block: "minecraft:chest".to_owned(), a: 1, b: 1 }]);

        // The hotbar's diamonds (menu slot 54) into the chest's first slot.
        let update = server.menu_batch(1, 1, &[click(54, ContainerInput::Pickup), click(0, ContainerInput::Pickup)], player(&inventory));
        apply(&mut inventory, &update);
        assert_eq!((update.ack, update.closed), (1, false));
        assert_eq!(update.player, vec![(0, None)]);
        assert_eq!(update.cursor, Some(None));
        assert_eq!(update.slots[0], Some(stack("minecraft:diamond", 10, 64)));
        assert_eq!(items(&server, pos)[0], Some(stack("minecraft:diamond", 10, 64)), "written to the block entity");
        let saved = server.take_block_entity_changes();
        assert!(saved.iter().any(|(p, tag)| *p == pos && tag.as_ref().and_then(|t| t.get("Items")).and_then(Tag::as_list).is_some_and(|list| list.len() == 1)), "and to the chunk's save: {saved:?}");
        // The logs (menu slot 27, inventory 9) shift-clicked in: the first
        // empty slot.
        let update = server.menu_batch(1, 2, &[click(27, ContainerInput::QuickMove)], player(&inventory));
        apply(&mut inventory, &update);
        assert_eq!(update.player, vec![(9, None)]);
        assert_eq!(items(&server, pos)[1], Some(stack("minecraft:oak_log", 5, 64)));
        // The diamonds shift-clicked out: the hotbar from its end first.
        let update = server.menu_batch(1, 3, &[click(0, ContainerInput::QuickMove)], player(&inventory));
        apply(&mut inventory, &update);
        assert_eq!(update.player, vec![(8, Some(stack("minecraft:diamond", 10, 64)))]);
        assert_eq!(items(&server, pos)[0], None);

        let update = server.menu_batch(1, 4, &[MenuInput::Close], player(&inventory));
        apply(&mut inventory, &update);
        assert!(update.closed && update.player.is_empty());
        assert_eq!(server.open_menu(), None);
        assert_eq!(server.level.take_sounds().iter().map(|s| s.event).collect::<Vec<_>>(), ["minecraft:block.chest.close"]);
        tick(&mut server, FEET);
        assert_eq!(server.take_block_events(), vec![crate::server::BlockEventView { pos, block: "minecraft:chest".to_owned(), a: 1, b: 0 }]);
        assert_eq!(inventory.slots[8], Some(stack("minecraft:diamond", 10, 64)));
        assert!(inventory.slots[0].is_none() && inventory.slots[9].is_none());
        assert_eq!(items(&server, pos)[1], Some(stack("minecraft:oak_log", 5, 64)), "the logs stay in the chest");
        // A batch for the closed menu changes nothing, and the next menu
        // has the next id.
        let stale = server.menu_batch(1, 5, &[click(1, ContainerInput::QuickMove)], player(&inventory));
        assert!(stale.closed && stale.player.is_empty());
        assert_eq!(items(&server, pos)[1], Some(stack("minecraft:oak_log", 5, 64)));
        let (_, updates) = server.use_block_with(pos, "north", player(&inventory));
        assert_eq!(updates[0].id, 2);
    }

    /// A double chest is one 54-slot menu: its right half's 27 slots, then
    /// its left half's. It sounds once, from between the halves, and both
    /// lids open; its title is a half's custom name.
    #[test]
    fn a_double_chest_has_54_slots_and_its_halves() {
        let Some((mut server, mut scene)) = world() else { return };
        // Facing north, a right half connects west to its left half.
        let (right, left) = ((9, 200, 8), (8, 200, 8));
        place(&mut server, &mut scene, right, Block::new("minecraft:chest").with("facing", "north").with("type", "right"));
        place(&mut server, &mut scene, left, Block::new("minecraft:chest").with("facing", "north").with("type", "left"));
        let name = Tag::Compound([("minecraft:custom_name".to_owned(), Tag::String("Loot".to_owned()))].into());
        server.level.apply_container_components(LevelPos::new(8, 200, 8), Some(&name));
        let half = |p: BlockPos| ContainerRef::Single(LevelPos::new(p.0, p.1, p.2), minecraftoss_world::level::container::Store::Chest);
        server.level.container_set_item(half(right), 0, LevelStack::new("minecraft:diamond", 3));
        server.level.container_set_item(half(left), 0, LevelStack::new("minecraft:emerald", 7));
        server.level.take_sounds();

        let inventory = Inventory::default();
        let (result, updates) = server.use_block_with(left, "north", player(&inventory));
        assert!(result.opened);
        let opening = &updates[0];
        assert_eq!(opening.open, Some(MenuOpen { kind: MenuKind::Generic { rows: 6 }, title: json!("Loot") }), "the second half's name");
        assert_eq!(opening.slots.len(), 54);
        assert_eq!(opening.slots[0].as_ref().map(|s| (s.id.as_str(), s.count)), Some(("minecraft:diamond", 3)), "the right half is first");
        assert_eq!(opening.slots[27].as_ref().map(|s| (s.id.as_str(), s.count)), Some(("minecraft:emerald", 7)));
        let sounds = server.level.take_sounds();
        assert_eq!(sounds.iter().map(|s| (s.event, s.position)).collect::<Vec<_>>(), [("minecraft:block.chest.open", [9.0, 200.5, 8.5])]);
        tick(&mut server, FEET);
        let mut lids: Vec<BlockPos> = server.take_block_events().iter().filter(|e| (e.a, e.b) == (1, 1)).map(|e| e.pos).collect();
        lids.sort();
        assert_eq!(lids, [left, right]);

        // The emeralds from slot 27 to slot 30: the left half's slot 3.
        let update = server.menu_batch(opening.id, 1, &[click(27, ContainerInput::Pickup), click(30, ContainerInput::Pickup)], player(&inventory));
        assert_eq!(update.slots[30].as_ref().map(|s| s.count), Some(7));
        assert_eq!(items(&server, left)[3].as_ref().map(|s| (s.id.as_str(), s.count)), Some(("minecraft:emerald", 7)));
        assert_eq!(items(&server, left)[0], None);
        assert_eq!(items(&server, right)[0].as_ref().map(|s| s.count), Some(3));
        // Shift-clicked from the player, stacks fill the right half first.
        let mut inventory = Inventory::default();
        inventory.slots[0] = Some(stack("minecraft:emerald", 5, 64));
        let update = server.menu_batch(opening.id, 2, &[click(81, ContainerInput::QuickMove)], player(&inventory));
        assert_eq!(update.player, vec![(0, None)]);
        assert_eq!(items(&server, left)[3].as_ref().map(|s| s.count), Some(12), "merged into the emeralds first");
    }

    /// A chest under a full block or a sitting cat does not open; an ender
    /// chest only minds the block above; a barrel opens regardless; a
    /// shulker box needs room for its lid.
    #[test]
    fn blocked_containers_refuse_to_open() {
        let Some((mut server, mut scene)) = world() else { return };
        let inventory = Inventory::default();
        let (chest, ender, barrel, shulker) = ((8, 200, 8), (10, 200, 8), (6, 200, 8), (8, 200, 6));
        place(&mut server, &mut scene, chest, Block::new("minecraft:chest"));
        place(&mut server, &mut scene, (8, 201, 8), Block::new("minecraft:stone"));
        let (result, updates) = server.use_block_with(chest, "north", player(&inventory));
        assert!(!result.opened && updates.is_empty() && result.stats.is_empty());
        assert!(server.level.take_sounds().is_empty());
        // Glass above lets it open.
        place(&mut server, &mut scene, (8, 201, 8), Block::new("minecraft:glass"));
        let (result, _) = server.use_block_with(chest, "north", player(&inventory));
        assert!(result.opened);
        server.menu_batch(1, 1, &[MenuInput::Close], player(&inventory));
        // A cat sitting on it does not.
        let cat = minecraftoss_core::snbt::parse_compound("{id:\"minecraft:cat\",Pos:[8.5d,201.1d,8.5d],Sitting:1b}").expect("a cat");
        server.dormant.push((minecraftoss_world::natural_spawner::tick::census_of(&cat), cat));
        let (result, _) = server.use_block_with(chest, "north", player(&inventory));
        assert!(!result.opened, "the cat sits on it");
        server.dormant.clear();

        place(&mut server, &mut scene, ender, Block::new("minecraft:ender_chest"));
        place(&mut server, &mut scene, (10, 201, 8), Block::new("minecraft:stone"));
        let (result, _) = server.use_block_with(ender, "north", player(&inventory));
        assert!(!result.opened);

        place(&mut server, &mut scene, barrel, Block::new("minecraft:barrel").with("facing", "up"));
        place(&mut server, &mut scene, (6, 201, 8), Block::new("minecraft:stone"));
        server.level.take_sounds();
        let (result, _) = server.use_block_with(barrel, "north", player(&inventory));
        assert!(result.opened, "a barrel always opens");
        assert_eq!(result.stats[0].1, "minecraft:open_barrel");
        let barrel_state = server.level.block(LevelPos::new(6, 200, 8));
        assert_eq!(server.level.registries().blocks.property(barrel_state, "open"), Some("true"), "and shows open");
        let sounds = server.level.take_sounds();
        assert_eq!(sounds.iter().map(|s| (s.event, s.position)).collect::<Vec<_>>(), [("minecraft:block.barrel.open", [6.5, 201.0, 8.5])], "from its open face");
        let id = server.open_menu().expect("open").0;
        server.menu_batch(id, 1, &[MenuInput::Close], player(&inventory));
        let barrel_state = server.level.block(LevelPos::new(6, 200, 8));
        assert_eq!(server.level.registries().blocks.property(barrel_state, "open"), Some("false"));

        place(&mut server, &mut scene, shulker, Block::new("minecraft:shulker_box").with("facing", "up"));
        place(&mut server, &mut scene, (8, 201, 6), Block::new("minecraft:oak_slab"));
        let (result, _) = server.use_block_with(shulker, "north", player(&inventory));
        assert!(!result.opened, "the slab is in the lid's way");
        place(&mut server, &mut scene, (8, 201, 6), Block::new("minecraft:oak_slab").with("type", "top"));
        let (result, _) = server.use_block_with(shulker, "north", player(&inventory));
        assert!(result.opened, "a top slab leaves the lid room");
    }

    /// Copper chests creak by their weathering, waxed or not.
    #[test]
    fn copper_chests_sound_by_their_weathering() {
        let Some((mut server, mut scene)) = world() else { return };
        let inventory = Inventory::default();
        for (x, block, open, close) in [
            (6, "minecraft:copper_chest", "minecraft:block.copper_chest.open", "minecraft:block.copper_chest.close"),
            (8, "minecraft:exposed_copper_chest", "minecraft:block.copper_chest.open", "minecraft:block.copper_chest.close"),
            (10, "minecraft:waxed_weathered_copper_chest", "minecraft:block.copper_chest_weathered.open", "minecraft:block.copper_chest_weathered.close"),
            (12, "minecraft:oxidized_copper_chest", "minecraft:block.copper_chest_oxidized.open", "minecraft:block.copper_chest_oxidized.close"),
            (14, "minecraft:trapped_chest", "minecraft:block.chest.open", "minecraft:block.chest.close"),
        ] {
            let pos = (x, 200, 8);
            place(&mut server, &mut scene, pos, Block::new(block));
            server.level.take_sounds();
            let mut near = player(&inventory);
            near.eye = [f64::from(x) + 0.5, 201.62, 10.5];
            let (result, updates) = server.use_block_with(pos, "north", near.clone());
            assert!(result.opened, "{block}");
            assert_eq!(updates[0].open.as_ref().map(|o| o.title.clone()), Some(json!({"translate": "container.chest"})));
            server.menu_batch(updates[0].id, 1, &[MenuInput::Close], near);
            assert_eq!(server.level.take_sounds().iter().map(|s| s.event).collect::<Vec<_>>(), [open, close], "{block}");
        }
    }

    /// A locked chest opens only for the stack its lock names, and tells
    /// the player otherwise.
    #[test]
    fn a_locked_chest_opens_for_its_key() {
        let Some((mut server, mut scene)) = world() else { return };
        let pos = (8, 200, 8);
        place(&mut server, &mut scene, pos, Block::new("minecraft:chest"));
        let lock = Tag::Compound([("items".to_owned(), Tag::String("minecraft:tripwire_hook".to_owned()))].into());
        let components = Tag::Compound([("minecraft:lock".to_owned(), lock)].into());
        server.level.apply_container_components(LevelPos::new(8, 200, 8), Some(&components));
        server.level.take_sounds();
        let mut inventory = Inventory::default();
        let (result, updates) = server.use_block_with(pos, "north", player(&inventory));
        assert!(!result.opened && updates.is_empty());
        assert_eq!(result.overlay, Some(json!({"translate": "container.isLocked", "with": [{"translate": "container.chest"}]})));
        assert_eq!(server.level.take_sounds().iter().map(|s| (s.event, s.volume, s.pitch)).collect::<Vec<_>>(), [("minecraft:block.chest.locked", 1.0, 1.0)]);
        inventory.slots[0] = Some(stack("minecraft:tripwire_hook", 1, 64));
        let (result, _) = server.use_block_with(pos, "north", player(&inventory));
        assert!(result.opened);
    }

    /// While a chest is open, what a hopper feeds it reaches the screen
    /// after the tick, with no player part; a tick that changes nothing
    /// sends nothing.
    #[test]
    fn a_hopper_feeding_an_open_chest_is_pushed() {
        let Some((mut server, mut scene)) = world() else { return };
        let (chest, hopper) = ((8, 200, 8), (8, 201, 8));
        place(&mut server, &mut scene, chest, Block::new("minecraft:chest"));
        place(&mut server, &mut scene, hopper, Block::new("minecraft:hopper"));
        let hopper_ref = ContainerRef::Single(LevelPos::new(8, 201, 8), minecraftoss_world::level::container::Store::Hopper);
        server.level.container_set_item(hopper_ref, 0, LevelStack::new("minecraft:diamond", 2));
        let (result, updates) = server.use_block_with(chest, "north", player(&Inventory::default()));
        assert!(result.opened);
        let id = updates[0].id;
        let mut pushed = Vec::new();
        for _ in 0..20 {
            pushed.extend(tick(&mut server, FEET));
        }
        let counts: Vec<u8> = pushed.iter().map(|u| u.slots[0].as_ref().map_or(0, |s| s.count)).collect();
        assert_eq!(counts, [1, 2], "one push per item the hopper moved: {pushed:?}");
        for update in &pushed {
            assert_eq!((update.id, update.ack, update.cursor.as_ref(), update.closing), (id, 0, None, false));
            assert!(update.player.is_empty());
        }
        // A batch acknowledges what the hopper did.
        let update = server.menu_batch(id, 1, &[], player(&Inventory::default()));
        assert_eq!(update.slots[0].as_ref().map(|s| s.count), Some(2));
        assert_eq!(tick(&mut server, FEET), None);
    }

    /// The ender chest shows the player's own ender inventory, from any
    /// ender chest, and hands its changes back for the client to save.
    #[test]
    fn the_ender_chest_round_trips_the_players_items() {
        let Some((mut server, mut scene)) = world() else { return };
        let (first, second) = ((8, 200, 8), (10, 200, 8));
        place(&mut server, &mut scene, first, Block::new("minecraft:ender_chest"));
        place(&mut server, &mut scene, second, Block::new("minecraft:ender_chest"));
        let sword = PlayerStack { id: "minecraft:diamond_sword".to_owned(), count: 1, max: 1, components: Some(json!({"minecraft:damage": 12})) };
        let mut saved = vec![None; ENDER_SLOTS];
        saved[0] = Some(stack("minecraft:ender_pearl", 4, 16));
        saved[26] = Some(sword.clone());
        server.set_ender_items(saved.clone());
        server.level.take_sounds();
        let mut inventory = Inventory::default();
        let (result, updates) = server.use_block_with(first, "north", player(&inventory));
        assert_eq!(result.stats[0].1, "minecraft:open_enderchest");
        let opening = &updates[0];
        assert_eq!(opening.open, Some(MenuOpen { kind: MenuKind::Generic { rows: 3 }, title: json!({"translate": "container.enderchest"}) }));
        assert_eq!(opening.slots, saved);
        assert_eq!(server.level.take_sounds().iter().map(|s| s.event).collect::<Vec<_>>(), ["minecraft:block.ender_chest.open"]);
        tick(&mut server, FEET);
        assert_eq!(server.take_block_events(), vec![crate::server::BlockEventView { pos: first, block: "minecraft:ender_chest".to_owned(), a: 1, b: 1 }]);

        let update = server.menu_batch(opening.id, 1, &[click(0, ContainerInput::QuickMove)], player(&inventory));
        apply(&mut inventory, &update);
        assert_eq!(inventory.slots[8], Some(stack("minecraft:ender_pearl", 4, 16)));
        let ender = update.ender.expect("the ender inventory changed");
        assert_eq!((ender[0].as_ref(), ender[26].as_ref()), (None, Some(&sword)));
        let update = server.menu_batch(opening.id, 2, &[MenuInput::Close], player(&inventory));
        assert!(update.closed && update.ender.is_none());
        assert_eq!(server.level.take_sounds().iter().map(|s| s.event).collect::<Vec<_>>(), ["minecraft:block.ender_chest.close"]);

        let (_, updates) = server.use_block_with(second, "north", player(&inventory));
        assert_eq!((updates[0].slots[0].as_ref(), updates[0].slots[26].as_ref()), (None, Some(&sword)), "the same items in the other chest");
        assert_eq!(server.ender_items()[26], Some(sword));
    }

    /// A broken shulker box drops itself with its contents and name, and
    /// placing that item fills the new box; in creative, only a box that
    /// holds something drops.
    #[test]
    fn a_shulker_box_keeps_its_contents_when_broken_and_placed() {
        let Some((mut server, mut scene)) = world() else { return };
        let (pos, again) = ((8, 200, 8), (10, 200, 8));
        let named = PlayerStack { id: "minecraft:red_shulker_box".to_owned(), count: 1, max: 1, components: Some(json!({"minecraft:custom_name": "Pack"})) };
        let block = Block::new("minecraft:red_shulker_box").with("facing", "up");
        scene.set(pos, Some(block.clone()));
        server.player_edit_block_by(pos, Some(&block), PlayerEdit::Place, Some(&EditBy::Placing(named)));
        let at = LevelPos::new(8, 200, 8);
        assert_eq!(server.level.block_entity(at).and_then(|t| t.get("CustomName")), Some(&Tag::String("Pack".to_owned())));

        let mut inventory = Inventory::default();
        inventory.slots[0] = Some(stack("minecraft:diamond", 10, 64));
        inventory.slots[1] = Some(stack("minecraft:shulker_box", 1, 1));
        let (result, updates) = server.use_block_with(pos, "north", player(&inventory));
        assert_eq!(result.stats[0].1, "minecraft:open_shulker_box");
        let opening = &updates[0];
        assert_eq!(opening.open, Some(MenuOpen { kind: MenuKind::ShulkerBox, title: json!("Pack") }));
        assert_eq!(server.level.take_sounds().iter().map(|s| s.event).collect::<Vec<_>>(), ["minecraft:block.shulker_box.open"]);
        // Diamonds go in; another shulker box does not.
        let update = server.menu_batch(opening.id, 1, &[click(54, ContainerInput::QuickMove), click(55, ContainerInput::QuickMove)], player(&inventory));
        apply(&mut inventory, &update);
        assert_eq!(update.player, vec![(0, None)]);
        assert_eq!(update.slots[0], Some(stack("minecraft:diamond", 10, 64)));
        assert_eq!(update.slots[1], None);
        // The lid opens over ten ticks, and closes.
        for _ in 0..11 {
            tick(&mut server, FEET);
        }
        assert_eq!(server.level.shulker_lid(at), LidStatus::Opened);
        server.menu_batch(opening.id, 2, &[MenuInput::Close], player(&inventory));
        for _ in 0..11 {
            tick(&mut server, FEET);
        }
        assert_eq!(server.level.shulker_lid(at), LidStatus::Closed);

        // Broken in survival: the box itself, with what it holds.
        server.level.entities.clear();
        scene.set(pos, None);
        server.player_edit_block_by(pos, None, PlayerEdit::Break, Some(&EditBy::Breaking(Breaker { tool: None, harvests: true, creative: false })));
        let drops: Vec<LevelStack> = server.level.entities.iter().filter_map(|e| e.item_data()).map(|d| d.stack.clone()).collect();
        let [drop] = &drops[..] else { panic!("one drop: {drops:?}") };
        let dropped = stacks::to_player(drop, |_| 1).expect("a stack");
        assert_eq!(dropped.id, "minecraft:red_shulker_box");
        assert_eq!(dropped.components, Some(json!({"minecraft:container": [{"slot": 0, "item": {"id": "minecraft:diamond", "count": 10}}], "minecraft:custom_name": "Pack"})));

        // Placed again, the box holds them.
        let block = Block::new("minecraft:red_shulker_box").with("facing", "up");
        scene.set(again, Some(block.clone()));
        server.player_edit_block_by(again, Some(&block), PlayerEdit::Place, Some(&EditBy::Placing(dropped)));
        assert_eq!(items(&server, again)[0], Some(stack("minecraft:diamond", 10, 64)));
        assert_eq!(server.level.block_entity(LevelPos::new(10, 200, 8)).and_then(|t| t.get("CustomName")), Some(&Tag::String("Pack".to_owned())));

        // In creative it drops only for holding something.
        server.level.entities.clear();
        scene.set(again, None);
        server.player_edit_block_by(again, None, PlayerEdit::Break, Some(&EditBy::Breaking(Breaker { tool: None, harvests: true, creative: true })));
        let drops: Vec<LevelStack> = server.level.entities.iter().filter_map(|e| e.item_data()).map(|d| d.stack.clone()).collect();
        assert_eq!(drops.len(), 1, "{drops:?}");
        assert!(drops[0].components.as_ref().and_then(|c| c.get("minecraft:container")).is_some());
        server.level.entities.clear();
        let block = Block::new("minecraft:shulker_box").with("facing", "up");
        place(&mut server, &mut scene, again, block);
        scene.set(again, None);
        server.player_edit_block_by(again, None, PlayerEdit::Break, Some(&EditBy::Breaking(Breaker { tool: None, harvests: true, creative: true })));
        assert!(server.level.entities.is_empty(), "an empty box drops nothing in creative");
    }

    /// A chest broken in survival drops its contents and itself, with its
    /// custom name.
    #[test]
    fn a_broken_chest_keeps_its_name() {
        let Some((mut server, mut scene)) = world() else { return };
        let pos = (8, 200, 8);
        place(&mut server, &mut scene, pos, Block::new("minecraft:chest"));
        let components = Tag::Compound([("minecraft:custom_name".to_owned(), Tag::String("Stash".to_owned()))].into());
        server.level.apply_container_components(LevelPos::new(8, 200, 8), Some(&components));
        server.level.container_set_item(ContainerRef::Single(LevelPos::new(8, 200, 8), minecraftoss_world::level::container::Store::Chest), 4, LevelStack::new("minecraft:apple", 2));
        scene.set(pos, None);
        server.player_edit_block_by(pos, None, PlayerEdit::Break, Some(&EditBy::Breaking(Breaker { tool: None, harvests: true, creative: false })));
        let mut drops: Vec<(String, Option<Tag>)> = server.level.entities.iter().filter_map(|e| e.item_data()).map(|d| (d.stack.id.clone(), d.stack.components.clone())).collect();
        drops.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(drops, [("minecraft:apple".to_owned(), None), ("minecraft:chest".to_owned(), Some(components))]);
    }

    /// A chest's loot table as generation leaves it (`container_loot` as a
    /// placed item gives it).
    fn give_loot(server: &mut ServerSim, pos: BlockPos, table: &str, seed: i64) {
        let loot = Tag::Compound([("loot_table".to_owned(), Tag::String(table.to_owned())), ("seed".to_owned(), Tag::Long(seed))].into());
        let components = Tag::Compound([("minecraft:container_loot".to_owned(), loot)].into());
        server.level.apply_container_components(LevelPos::new(pos.0, pos.1, pos.2), Some(&components));
    }

    fn has_loot_table(server: &ServerSim, pos: BlockPos) -> bool {
        server.level.block_entity(LevelPos::new(pos.0, pos.1, pos.2)).and_then(|t| t.get("LootTable")).is_some()
    }

    /// A chest with a loot table is filled from it as it opens, both halves
    /// of a double chest, and then keeps its items and no table, so it
    /// saves as vanilla's does. The seed decides the loot: two chests with
    /// the same table and seed hold the same.
    #[test]
    fn loot_chests_fill_as_they_open() {
        let Some((mut server, mut scene)) = world() else { return };
        let (right, left, other) = ((9, 200, 8), (8, 200, 8), (8, 200, 4));
        place(&mut server, &mut scene, right, Block::new("minecraft:chest").with("facing", "north").with("type", "right"));
        place(&mut server, &mut scene, left, Block::new("minecraft:chest").with("facing", "north").with("type", "left"));
        place(&mut server, &mut scene, other, Block::new("minecraft:chest"));
        give_loot(&mut server, right, "minecraft:chests/simple_dungeon", 1234);
        give_loot(&mut server, left, "minecraft:chests/simple_dungeon", 5678);
        give_loot(&mut server, other, "minecraft:chests/simple_dungeon", 1234);
        let inventory = Inventory::default();
        let (result, updates) = server.use_block_with(left, "north", player(&inventory));
        assert!(result.opened);
        let opening = &updates[0];
        assert!(opening.slots[..27].iter().any(Option::is_some) && opening.slots[27..].iter().any(Option::is_some), "both halves: {:?}", opening.slots);
        assert!(!has_loot_table(&server, right) && !has_loot_table(&server, left));
        assert_eq!(opening.slots[..27], items(&server, right)[..], "the menu shows what the chest holds");
        let saved = server.take_block_entity_changes();
        let left_saved = saved.iter().find(|(p, _)| *p == left).and_then(|(_, tag)| tag.as_ref()).expect("the left half saves");
        assert!(left_saved.get("LootTable").is_none() && left_saved.get("Items").and_then(Tag::as_list).is_some_and(|list| !list.is_empty()));
        server.menu_batch(opening.id, 1, &[MenuInput::Close], player(&inventory));
        let (_, updates) = server.use_block_with(other, "north", player(&inventory));
        assert_eq!(updates[0].slots, items(&server, right), "the same seed, the same loot");
    }

    /// A loot chest is filled as soon as anything takes from it: a hopper
    /// below pulls its loot, and a broken one drops it.
    #[test]
    fn hoppers_and_breaking_unpack_loot_chests() {
        let Some((mut server, mut scene)) = world() else { return };
        let (chest, hopper, broken) = ((8, 201, 8), (8, 200, 8), (4, 200, 8));
        place(&mut server, &mut scene, hopper, Block::new("minecraft:hopper"));
        place(&mut server, &mut scene, chest, Block::new("minecraft:chest"));
        give_loot(&mut server, chest, "minecraft:chests/simple_dungeon", 99);
        tick(&mut server, FEET);
        assert!(!has_loot_table(&server, chest));
        assert!(items(&server, hopper).iter().flatten().map(|s| s.count).sum::<u8>() == 1, "the hopper took one item: {:?}", items(&server, hopper));
        place(&mut server, &mut scene, broken, Block::new("minecraft:chest"));
        give_loot(&mut server, broken, "minecraft:chests/simple_dungeon", 99);
        let before = server.level.entities.iter().filter(|e| e.item_data().is_some()).count();
        scene.set(broken, None);
        server.player_edit_block(broken, None, PlayerEdit::Break);
        let dropped = server.level.entities.iter().filter(|e| e.item_data().is_some()).count() - before;
        assert!(dropped > 1, "the loot and the chest drop: {dropped}");
    }

    /// Walking out of reach closes the chest (its sound and lid) and asks
    /// the client to close; the client's next batch is ignored but for the
    /// carried stack, which goes back.
    #[test]
    fn still_valid_closes_the_menu_when_the_player_walks_away() {
        let Some((mut server, mut scene)) = world() else { return };
        let pos = (8, 200, 8);
        place(&mut server, &mut scene, pos, Block::new("minecraft:chest"));
        let mut inventory = Inventory::default();
        inventory.slots[0] = Some(stack("minecraft:diamond", 10, 64));
        let (_, updates) = server.use_block_with(pos, "north", player(&inventory));
        let id = updates[0].id;
        // The diamonds on the cursor.
        let update = server.menu_batch(id, 1, &[click(54, ContainerInput::Pickup)], player(&inventory));
        apply(&mut inventory, &update);
        assert_eq!(inventory.cursor, Some(stack("minecraft:diamond", 10, 64)));
        server.level.take_sounds();
        // Eight blocks off is still in reach; ten is not.
        assert_eq!(tick(&mut server, [8.5, 200.0, 16.5]), None);
        let update = tick(&mut server, [8.5, 200.0, 18.5]).expect("the menu closes");
        assert!(update.closing && !update.closed && update.cursor.is_none());
        assert_eq!(server.level.take_sounds().iter().map(|s| s.event).collect::<Vec<_>>(), ["minecraft:block.chest.close"]);
        assert_eq!(tick(&mut server, [8.5, 200.0, 18.5]), None, "asked once");
        let update = server.menu_batch(id, 2, &[click(0, ContainerInput::Pickup), MenuInput::Close], player(&inventory));
        apply(&mut inventory, &update);
        assert!(update.closed);
        assert_eq!(inventory.slots[0], Some(stack("minecraft:diamond", 10, 64)), "the carried stack went back");
        assert_eq!(inventory.cursor, None);
        assert_eq!(items(&server, pos), vec![None; 27], "the click was ignored");
        assert!(server.level.take_sounds().is_empty(), "closed once");

        // A chest broken while open closes its menu too.
        let (_, updates) = server.use_block_with(pos, "north", player(&inventory));
        let id = updates[0].id;
        scene.set(pos, None);
        server.player_edit_block(pos, None, PlayerEdit::Break);
        let update = tick(&mut server, FEET).expect("the menu closes");
        assert!(update.closing);
        assert!(server.menu_batch(id, 1, &[MenuInput::Close], player(&inventory)).closed);
    }

    /// A trapped chest's openers are its signal; the openers are rechecked
    /// every 5 ticks, which keeps a player's chest open and puts right a
    /// count with no player behind it.
    #[test]
    fn trapped_chests_signal_and_openers_are_rechecked() {
        let Some((mut server, mut scene)) = world() else { return };
        let (trapped, chest) = ((8, 200, 8), (10, 200, 8));
        place(&mut server, &mut scene, trapped, Block::new("minecraft:trapped_chest"));
        place(&mut server, &mut scene, chest, Block::new("minecraft:chest"));
        // A lamp under the stone the chest stands on.
        place(&mut server, &mut scene, (8, 199, 8), Block::new("minecraft:stone"));
        place(&mut server, &mut scene, (8, 198, 8), Block::new("minecraft:redstone_lamp"));
        let lamp_lit = |server: &ServerSim| server.level.registries().blocks.property(server.level.block(LevelPos::new(8, 198, 8)), "lit") == Some("true");
        let at = LevelPos::new(8, 200, 8);
        assert_eq!(server.level.signal(at, Direction::North), 0);
        assert!(!lamp_lit(&server));
        let (result, updates) = server.use_block_with(trapped, "north", player(&Inventory::default()));
        assert_eq!(result.stats[0].1, "minecraft:trigger_trapped_chest");
        assert_eq!(server.level.signal(at, Direction::North), 1, "one opener");
        assert!(lamp_lit(&server), "the block below is powered through");
        server.level.take_sounds();
        for _ in 0..12 {
            assert_eq!(tick(&mut server, FEET), None);
        }
        assert!(server.level.take_sounds().is_empty(), "the rechecks keep it open");
        assert_eq!(server.level.opener_count(at), 1);
        server.menu_batch(updates[0].id, 1, &[MenuInput::Close], player(&Inventory::default()));
        assert_eq!(server.level.signal(at, Direction::North), 0);
        for _ in 0..4 {
            tick(&mut server, FEET);
        }
        assert!(!lamp_lit(&server), "and goes dark");
        server.level.take_sounds();

        // An opener no player stands behind is gone by the recheck.
        let other = LevelPos::new(10, 200, 8);
        server.level.start_open(other, 4.5);
        assert_eq!(server.level.take_sounds().iter().map(|s| s.event).collect::<Vec<_>>(), ["minecraft:block.chest.open"]);
        for _ in 0..6 {
            tick(&mut server, FEET);
        }
        assert_eq!(server.level.opener_count(other), 0);
        assert_eq!(server.level.take_sounds().iter().map(|s| s.event).collect::<Vec<_>>(), ["minecraft:block.chest.close"]);
    }

    #[test]
    fn menu_effects_happen_at_the_player_or_the_menus_block() {
        let Some((mut server, _)) = world() else { return };
        let block = LevelPos::new(3, 70, 5);
        let feet = [8.5, 64.0, 8.5];
        let mut inventory = Inventory::default();
        let mut random = LegacyRandom::new(0);
        let mut cx = MenuContext::new(&mut inventory, &mut random);
        // A furnace's output at the player, the grindstone's at its block.
        cx.xp_orbs = vec![(MenuPlace::Player, 3), (MenuPlace::Block, 7)];
        cx.level_events = vec![1042];
        cx.sounds = vec![("minecraft:ui.stonecutter.take_result", 1.0, 1.0)];
        server.menu_effects(&mut cx, Some(block), feet);
        let orbs: Vec<([f64; 3], i32)> = server.orbs().iter().map(|orb| (orb.position, orb.value)).collect();
        assert_eq!(orbs, [(feet, 3), ([3.5, 70.5, 5.5], 7)]);
        assert_eq!(server.take_level_events(), [((3, 70, 5), 1042, 0)]);
        let sounds: Vec<(&str, [f64; 3])> = server.level.take_sounds().iter().map(|s| (s.event, s.position)).collect();
        assert_eq!(sounds, [("minecraft:ui.stonecutter.take_result", [3.5, 70.5, 5.5])]);
        assert!(cx.xp_orbs.is_empty() && cx.level_events.is_empty() && cx.sounds.is_empty());
    }

    #[test]
    fn reach_is_from_the_eye_to_the_nearest_point() {
        let pos = LevelPos::new(0, 64, 0);
        assert!(within_reach(pos, [0.5, 65.0, 8.9], false), "7.9 from the block's side");
        assert!(!within_reach(pos, [0.5, 65.0, 9.5], false), "8.5 is too far in survival");
        assert!(within_reach(pos, [0.5, 65.0, 9.5], true), "but not in creative");
        assert!(within_reach(pos, [0.5, 64.5, 0.5], false), "inside");
    }

    #[test]
    fn locks_open_for_their_key() {
        let key = |name: &str| PlayerStack { id: "minecraft:stick".to_owned(), count: 1, max: 64, components: Some(json!({"minecraft:custom_name": name})) };
        let lock = Tag::Compound([("items".to_owned(), Tag::String("minecraft:stick".to_owned())), ("components".to_owned(), Tag::Compound([("minecraft:custom_name".to_owned(), Tag::String("Key".to_owned()))].into()))].into());
        assert!(unlocks(&lock, Some(&key("Key"))));
        assert!(!unlocks(&lock, Some(&key("Not the key"))));
        assert!(!unlocks(&lock, None));
        assert!(unlocks(&Tag::Compound(Default::default()), None), "an empty predicate takes anything");
    }

    #[test]
    fn menu_kinds_name_vanilla_menu_types() {
        for (kind, own) in [(MenuKind::Generic { rows: 3 }, 27), (MenuKind::Generic { rows: 6 }, 54), (MenuKind::Generic3x3, 9), (MenuKind::Hopper, 5), (MenuKind::ShulkerBox, 27), (MenuKind::Furnace, 3), (MenuKind::BlastFurnace, 3), (MenuKind::Smoker, 3), (MenuKind::Crafter, 10)] {
            let menu = kind.menu(Vec::new());
            assert_eq!(menu.menu_type(), kind.menu_type());
            assert_eq!(menu.own().len(), own);
            assert_eq!(menu.slots().len(), own + 36);
        }
    }

    /// The recipe book of the game data's JAR, with its item catalog, given
    /// to the server and the player; none without them.
    fn cooking(server: &mut ServerSim, inventory: &mut Inventory) -> Option<std::sync::Arc<RecipeBook>> {
        let root = std::path::PathBuf::from(std::env::var("MINECRAFTOSS_ROOT").ok()?);
        let catalog = minecraftoss_player::item_catalog::ItemCatalog::from_path(&root.join("artifacts/item-catalog/26.3.json")).ok()?;
        let recipes = std::sync::Arc::new(RecipeBook::from_jar(&root.join("client.jar")).ok()?.with_item_catalog(std::sync::Arc::new(catalog)));
        server.set_recipe_book(recipes.clone());
        inventory.recipes = recipes.clone();
        Some(recipes)
    }

    fn lit(server: &ServerSim, pos: BlockPos) -> Option<&str> {
        server.level.registries().blocks.property(server.level.block(LevelPos::new(pos.0, pos.1, pos.2)), "lit")
    }

    /// A furnace opens; iron ore and coal go in by clicks and shift-clicks;
    /// it lights, cooks each ore in 200 ticks (the menu showing its
    /// progress), relights from the next coal, and the shift-clicked iron
    /// pays the recipe's experience at the player and unlocks it. Another
    /// furnace, broken full, pays at its centre.
    #[test]
    fn a_furnace_smelts_iron_and_pays_its_experience() {
        let Some((mut server, mut scene)) = world() else { return };
        let mut inventory = Inventory::default();
        let Some(recipes) = cooking(&mut server, &mut inventory) else { return };
        let (pos, other) = ((8, 200, 8), (9, 200, 8));
        place(&mut server, &mut scene, pos, Block::new("minecraft:furnace"));
        place(&mut server, &mut scene, other, Block::new("minecraft:furnace"));
        inventory.slots[0] = Some(recipes.stack("minecraft:iron_ore", 20));
        inventory.slots[1] = Some(recipes.stack("minecraft:coal", 4));

        // The other furnace first: half the ore and the coal, by
        // right-clicks.
        let (_, updates) = server.use_block_with(other, "north", player(&inventory));
        let id = updates[0].id;
        let update = server.menu_batch(id, 1, &[MenuInput::Click { slot: 30, button: 1, kind: ContainerInput::Pickup }, click(0, ContainerInput::Pickup), MenuInput::Click { slot: 31, button: 1, kind: ContainerInput::Pickup }, click(1, ContainerInput::Pickup), MenuInput::Close], player(&inventory));
        apply(&mut inventory, &update);
        assert_eq!(items(&server, other)[..2], [Some(recipes.stack("minecraft:iron_ore", 10)), Some(recipes.stack("minecraft:coal", 2))]);

        let (result, updates) = server.use_block_with(pos, "north", player(&inventory));
        assert!(result.opened);
        assert_eq!(result.stats, vec![("minecraft:custom".to_owned(), "minecraft:interact_with_furnace".to_owned(), 1)]);
        let [opening] = &updates[..] else { panic!("one opening: {updates:?}") };
        assert_eq!(opening.open, Some(MenuOpen { kind: MenuKind::Furnace, title: json!({"translate": "container.furnace"}) }));
        assert_eq!((opening.slots.len(), &opening.data[..]), (3, &[0, 0, 0, 0][..]));
        // The ore shift-clicked into the ingredient, the coal into the fuel.
        let update = server.menu_batch(opening.id, 1, &[click(30, ContainerInput::QuickMove), click(31, ContainerInput::QuickMove)], player(&inventory));
        apply(&mut inventory, &update);
        assert_eq!(update.player, vec![(0, None), (1, None)]);
        assert_eq!(update.slots[..2], [Some(recipes.stack("minecraft:iron_ore", 10)), Some(recipes.stack("minecraft:coal", 2))]);
        assert_eq!(update.data, [0, 0, 0, 200], "a new ingredient sets the cooking time");

        let shown = tick(&mut server, FEET).expect("the furnace lights");
        assert_eq!((&shown.data[..], shown.cursor.as_ref()), (&[1600, 1600, 1, 200][..], None));
        assert_eq!(shown.slots[1], Some(recipes.stack("minecraft:coal", 1)));
        assert_eq!((lit(&server, pos), lit(&server, other)), (Some("true"), Some("true")));
        for _ in 1..2000 {
            tick(&mut server, FEET);
        }
        let done = items(&server, pos);
        assert_eq!(done, [None, None, Some(recipes.stack("minecraft:iron_ingot", 10))], "the second coal lit when the first ran out");
        let recipe = "minecraft:iron_ingot_from_smelting_iron_ore";
        let used = |server: &ServerSim, p: BlockPos| server.level.block_entity(LevelPos::new(p.0, p.1, p.2)).and_then(|t| t.get("RecipesUsed")).and_then(|r| r.get(recipe)).and_then(Tag::as_i64);
        assert_eq!(used(&server, pos), Some(10));

        // The iron shift-clicked out: 10 times 0.7 experience at the feet.
        server.level.entities.clear();
        let update = server.menu_batch(opening.id, 2, &[click(2, ContainerInput::QuickMove)], player(&inventory));
        apply(&mut inventory, &update);
        assert_eq!(update.player, vec![(8, Some(recipes.stack("minecraft:iron_ingot", 10)))]);
        assert_eq!(update.slots[2], None);
        let orbs: Vec<([f64; 3], i32)> = server.orbs().iter().map(|orb| (orb.position, orb.value)).collect();
        assert_eq!(orbs.iter().map(|(_, value)| value).sum::<i32>(), 7);
        assert!(orbs.iter().all(|(at, _)| *at == FEET), "{orbs:?}");
        assert!(update.unlocked.contains(&recipe.to_owned()), "{:?}", update.unlocked);
        assert!(update.stats.contains(&(minecraftoss_player::statistics::CRAFTED.to_owned(), "minecraft:iron_ingot".to_owned(), 10)), "{:?}", update.stats);
        assert_eq!(used(&server, pos), None, "the furnace forgets what it paid");
        // Nothing to burn: it goes out with the coal.
        server.menu_batch(opening.id, 3, &[MenuInput::Close], player(&inventory));
        for _ in 0..1300 {
            tick(&mut server, FEET);
        }
        assert_eq!((lit(&server, pos), lit(&server, other)), (Some("false"), Some("false")));

        // The other furnace, broken with its iron: the experience at its
        // centre, and its items dropped.
        server.level.entities.clear();
        scene.set(other, None);
        server.player_edit_block_by(other, None, PlayerEdit::Break, Some(&EditBy::Breaking(Breaker { tool: None, harvests: true, creative: false })));
        let orbs: Vec<([f64; 3], i32)> = server.orbs().iter().map(|orb| (orb.position, orb.value)).collect();
        assert_eq!(orbs.iter().map(|(_, value)| value).sum::<i32>(), 7);
        assert!(orbs.iter().all(|(at, _)| *at == [9.5, 200.5, 8.5]), "{orbs:?}");
        let drops: Vec<String> = server.level.entities.iter().filter_map(|e| e.item_data()).map(|d| format!("{} {}", d.stack.id, d.stack.count)).collect();
        assert!(drops.contains(&"minecraft:iron_ingot 10".to_owned()), "{drops:?}");
    }

    /// Hoppers reach a furnace by its faces: the top feeds the ingredient,
    /// a side the fuel, and the bottom takes the result but not the fuel.
    /// A comparator reads it as a container.
    #[test]
    fn hoppers_feed_and_empty_a_furnace_by_its_faces() {
        let Some((mut server, mut scene)) = world() else { return };
        let mut inventory = Inventory::default();
        let Some(recipes) = cooking(&mut server, &mut inventory) else { return };
        let pos = (8, 200, 8);
        place(&mut server, &mut scene, pos, Block::new("minecraft:blast_furnace"));
        place(&mut server, &mut scene, (8, 201, 8), Block::new("minecraft:hopper").with("facing", "down"));
        place(&mut server, &mut scene, (9, 200, 8), Block::new("minecraft:hopper").with("facing", "west"));
        place(&mut server, &mut scene, (8, 199, 8), Block::new("minecraft:hopper").with("facing", "down"));
        server.level.replace_block_item(LevelPos::new(8, 201, 8), 0, LevelStack::new("minecraft:iron_ore", 2));
        server.level.replace_block_item(LevelPos::new(9, 200, 8), 0, LevelStack::new("minecraft:coal", 1));
        place(&mut server, &mut scene, (7, 199, 8), Block::new("minecraft:stone"));
        place(&mut server, &mut scene, (7, 200, 8), Block::new("minecraft:comparator").with("facing", "east"));
        for _ in 0..220 {
            tick(&mut server, FEET);
        }
        // A blast furnace cooks in half the time: both ores are done.
        assert_eq!(items(&server, (8, 199, 8))[0], Some(recipes.stack("minecraft:iron_ingot", 2)));
        let furnace = items(&server, pos);
        assert_eq!(furnace, [None, None, None], "the coal burned; nothing else came through the sides");
        assert_eq!(lit(&server, pos), Some("true"));
        // A bucket in the fuel slot is taken from below; coal is not.
        server.level.replace_block_item(LevelPos::new(8, 200, 8), 1, LevelStack::new("minecraft:coal", 1));
        server.level.replace_block_item(LevelPos::new(8, 199, 8), 0, LevelStack::empty());
        for _ in 0..20 {
            tick(&mut server, FEET);
        }
        assert_eq!(items(&server, pos)[1], Some(recipes.stack("minecraft:coal", 1)));
        assert_eq!(server.level.comparator_output_at(LevelPos::new(7, 200, 8)), 1, "one coal of 64, in three slots");
        server.level.replace_block_item(LevelPos::new(8, 200, 8), 1, LevelStack::new("minecraft:bucket", 1));
        for _ in 0..20 {
            tick(&mut server, FEET);
        }
        assert_eq!(items(&server, pos)[1], None);
        assert_eq!(items(&server, (8, 199, 8))[0], Some(recipes.stack("minecraft:bucket", 1)));
    }

    fn property(server: &ServerSim, pos: BlockPos, name: &str) -> Option<String> {
        server.level.registries().blocks.property(server.level.block(LevelPos::new(pos.0, pos.1, pos.2)), name).map(str::to_owned)
    }

    /// The items thrown into the level: each item's total.
    fn thrown(server: &ServerSim) -> std::collections::BTreeMap<String, i32> {
        let mut totals = std::collections::BTreeMap::new();
        for data in server.level.entities.iter().filter_map(|e| e.item_data()) {
            *totals.entry(data.stack.id.clone()).or_insert(0) += data.stack.count;
        }
        totals
    }

    /// A crafter opens, counting no statistic; a cake's ingredients go in
    /// by shift-clicks and drags, and the menu shows the cake. Powered, it
    /// crafts 4 ticks later: the cake and the three buckets are thrown from
    /// its front, each with the craft's sound and smoke, the grid empties,
    /// and `crafting` shows for 6 ticks. An empty grid fails.
    #[test]
    fn a_powered_crafter_throws_its_cake_and_buckets() {
        let Some((mut server, mut scene)) = world() else { return };
        let mut inventory = Inventory::default();
        let Some(recipes) = cooking(&mut server, &mut inventory) else { return };
        let pos = (8, 200, 8);
        place(&mut server, &mut scene, pos, Block::new("minecraft:crafter"));
        for slot in 0..3 {
            inventory.slots[slot] = Some(recipes.stack("minecraft:milk_bucket", 1));
        }
        inventory.slots[3] = Some(recipes.stack("minecraft:sugar", 2));
        inventory.slots[4] = Some(recipes.stack("minecraft:egg", 1));
        inventory.slots[5] = Some(recipes.stack("minecraft:wheat", 3));
        let (result, updates) = server.use_block_with(pos, "north", player(&inventory));
        assert!(result.opened && result.stats.is_empty(), "{result:?}");
        let [opening] = &updates[..] else { panic!("one opening: {updates:?}") };
        assert_eq!(opening.open, Some(MenuOpen { kind: MenuKind::Crafter, title: json!({"translate": "container.crafter"}) }));
        assert_eq!((opening.slots.len(), &opening.data[..]), (10, &[0; 10][..]));
        let inputs = [
            click(36, ContainerInput::QuickMove),
            click(37, ContainerInput::QuickMove),
            click(38, ContainerInput::QuickMove),
            click(39, ContainerInput::Pickup),
            MenuInput::Drag { button: 1, slots: vec![3, 5] },
            click(40, ContainerInput::QuickMove),
            click(41, ContainerInput::Pickup),
            MenuInput::Drag { button: 0, slots: vec![6, 7, 8] },
        ];
        let update = server.menu_batch(opening.id, 1, &inputs, player(&inventory));
        apply(&mut inventory, &update);
        assert!(inventory.slots[..6].iter().all(Option::is_none) && inventory.cursor.is_none());
        let shown: Vec<Option<(String, u8)>> = update.slots.iter().map(|s| s.as_ref().map(|s| (s.id.clone(), s.count))).collect();
        let at = |id: &str| Some((format!("minecraft:{id}"), 1));
        assert_eq!(shown, [at("milk_bucket"), at("milk_bucket"), at("milk_bucket"), at("sugar"), at("egg"), at("sugar"), at("wheat"), at("wheat"), at("wheat"), at("cake")]);
        server.menu_batch(opening.id, 2, &[MenuInput::Close], player(&inventory));

        let _ = server.take_level_events();
        place(&mut server, &mut scene, (9, 200, 8), Block::new("minecraft:redstone_block"));
        assert_eq!(property(&server, pos, "triggered").as_deref(), Some("true"));
        for _ in 0..3 {
            tick(&mut server, FEET);
        }
        assert!(thrown(&server).is_empty(), "the craft waits 4 ticks");
        tick(&mut server, FEET);
        let expected: std::collections::BTreeMap<String, i32> = [("minecraft:bucket".to_owned(), 3), ("minecraft:cake".to_owned(), 1)].into();
        assert_eq!(thrown(&server), expected);
        assert!(items(&server, pos).iter().all(Option::is_none), "one of each went");
        // Out of the front (north, data 2): the cake, then each bucket.
        assert_eq!(server.take_level_events(), [((8, 200, 8), 1049, 0), ((8, 200, 8), 2010, 2)].repeat(4));
        let items_at: Vec<[f64; 3]> = server.level.entities.iter().filter(|e| e.item_data().is_some()).map(|e| e.pos).collect();
        assert!(items_at.iter().all(|p| p[2] < 8.0), "{items_at:?}");
        assert_eq!(property(&server, pos, "crafting").as_deref(), Some("true"));
        for _ in 0..4 {
            tick(&mut server, FEET);
        }
        assert_eq!(property(&server, pos, "crafting").as_deref(), Some("true"));
        tick(&mut server, FEET);
        assert_eq!(property(&server, pos, "crafting").as_deref(), Some("false"));

        // Unpowered and powered again: nothing to craft.
        scene.set((9, 200, 8), None);
        server.player_edit_block((9, 200, 8), None, PlayerEdit::Break);
        assert_eq!(property(&server, pos, "triggered").as_deref(), Some("false"));
        place(&mut server, &mut scene, (9, 200, 8), Block::new("minecraft:redstone_block"));
        for _ in 0..4 {
            tick(&mut server, FEET);
        }
        assert_eq!(server.take_level_events(), [((8, 200, 8), 1050, 0)]);
        assert_eq!(property(&server, pos, "crafting").as_deref(), Some("false"));
    }

    /// A crafter's menu toggles only empty slots, in its data and in the
    /// block entity; a hopper above fills the enabled slots evenly; the
    /// menu shows what they craft, and a comparator counts the slots
    /// filled or disabled.
    #[test]
    fn crafter_slots_toggle_and_hoppers_fill_them_evenly() {
        let Some((mut server, mut scene)) = world() else { return };
        let mut inventory = Inventory::default();
        let Some(recipes) = cooking(&mut server, &mut inventory) else { return };
        let pos = (8, 200, 8);
        place(&mut server, &mut scene, pos, Block::new("minecraft:crafter"));
        place(&mut server, &mut scene, (8, 201, 8), Block::new("minecraft:hopper").with("facing", "down"));
        place(&mut server, &mut scene, (7, 199, 8), Block::new("minecraft:stone"));
        place(&mut server, &mut scene, (7, 200, 8), Block::new("minecraft:comparator").with("facing", "east"));
        inventory.slots[0] = Some(recipes.stack("minecraft:oak_planks", 1));
        let (_, updates) = server.use_block_with(pos, "north", player(&inventory));
        let id = updates[0].id;
        let mut inputs = vec![click(36, ContainerInput::Pickup), click(1, ContainerInput::Pickup)];
        inputs.extend([0, 1, 2, 3, 5, 6, 8].map(|slot| MenuInput::SlotState { slot, enabled: false }));
        let update = server.menu_batch(id, 1, &inputs, player(&inventory));
        apply(&mut inventory, &update);
        assert_eq!(update.data, [1, 0, 1, 1, 0, 1, 1, 0, 1, 0], "slot 1 holds a plank");
        assert_eq!(server.level.crafter_data(LevelPos::new(8, 200, 8)), Some([1, 0, 1, 1, 0, 1, 1, 0, 1, 0]));
        assert_eq!(update.slots[9].as_ref().map(|s| (s.id.as_str(), s.count)), Some(("minecraft:oak_button", 1)), "one plank is a button");
        // Enabled again, then disabled: the last state stays.
        let update = server.menu_batch(id, 2, &[MenuInput::SlotState { slot: 0, enabled: true }, MenuInput::SlotState { slot: 0, enabled: false }], player(&inventory));
        assert_eq!(update.data[0], 1);

        // The hopper's plank skips slot 1, which a later slot is emptier
        // than, for slot 4: two planks in a column make sticks.
        server.level.replace_block_item(LevelPos::new(8, 201, 8), 0, LevelStack::new("minecraft:oak_planks", 1));
        let mut shown = None;
        for _ in 0..20 {
            shown = tick(&mut server, FEET).or(shown);
        }
        let grid = items(&server, pos);
        assert_eq!((&grid[1], &grid[4]), (&Some(recipes.stack("minecraft:oak_planks", 1)), &Some(recipes.stack("minecraft:oak_planks", 1))));
        let shown = shown.expect("the tick sends the grid");
        assert_eq!(shown.slots[9].as_ref().map(|s| (s.id.as_str(), s.count)), Some(("minecraft:stick", 4)));
        assert_eq!(server.level.comparator_output_at(LevelPos::new(7, 200, 8)), 8, "six disabled, two filled");
    }
}
