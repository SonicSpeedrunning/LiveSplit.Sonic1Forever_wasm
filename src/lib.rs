#![no_std]
#![feature(type_alias_impl_trait, const_async_blocks)]
#![warn(
    clippy::complexity,
    clippy::correctness,
    clippy::perf,
    clippy::style,
    clippy::undocumented_unsafe_blocks,
    rust_2018_idioms
)]

use asr::{
    file_format::pe,
    future::{next_tick, retry},
    settings::Gui,
    signature::Signature,
    timer::{self, TimerState},
    watcher::Watcher,
    Address, Address32, Process,
};

asr::panic_handler!();
asr::async_main!(nightly);

const PROCESS_NAMES: &[&str] = &["SonicForever.exe"];

async fn main() {
    let mut settings = Settings::register();

    loop {
        // Hook to the target process
        let process = retry(|| PROCESS_NAMES.iter().find_map(|&name| Process::attach(name))).await;

        process
            .until_closes(async {
                // Once the target has been found and attached to, set up some default watchers
                let mut watchers = Watchers::default();

                // Perform memory scanning to look for the addresses we need
                let addresses = Addresses::init(&process).await;

                loop {
                    // Splitting logic. Adapted from OG LiveSplit:
                    // Order of execution
                    // 1. update() will always be run first. There are no conditions on the execution of this action.
                    // 2. If the timer is currently either running or paused, then the isLoading, gameTime, and reset actions will be run.
                    // 3. If reset does not return true, then the split action will be run.
                    // 4. If the timer is currently not running (and not paused), then the start action will be run.
                    settings.update();
                    update_loop(&process, &addresses, &mut watchers);

                    let timer_state = timer::state();
                    if timer_state == TimerState::Running || timer_state == TimerState::Paused {
                        if reset(&watchers, &settings, &addresses) {
                            timer::reset()
                        } else if split(&watchers, &settings) {
                            timer::split()
                        }
                    }

                    if timer::state() == TimerState::NotRunning
                        && start(&watchers, &settings, &addresses)
                    {
                        timer::start();
                    }

                    next_tick().await;
                }
            })
            .await;
    }
}

#[derive(asr::settings::Gui)]
struct Settings {
    #[default = true]
    /// Start --> New Game
    start_clean_save: bool,
    #[default = true]
    /// Start --> New Game+
    start_new_game_plus: bool,
    #[default = true]
    /// Reset --> Enable auto reset
    reset: bool,
    #[default = true]
    /// Green Hill Zone - Act 1
    green_hill_1: bool,
    #[default = true]
    /// Green Hill Zone - Act 2
    green_hill_2: bool,
    #[default = true]
    /// Green Hill Zone - Act 3
    green_hill_3: bool,
    #[default = true]
    /// Marble Zone - Act 1
    marble_1: bool,
    #[default = true]
    /// Marble Zone - Act 2
    marble_2: bool,
    #[default = true]
    /// Marble Zone - Act 3
    marble_3: bool,
    #[default = true]
    /// Spring Yard Zone - Act 1
    spring_yard_1: bool,
    #[default = true]
    /// Spring Yard Zone - Act 2
    spring_yard_2: bool,
    #[default = true]
    /// Spring Yard Zone - Act 3
    spring_yard_3: bool,
    #[default = true]
    /// Labyrinth Zone - Act 1
    labyrinth_1: bool,
    #[default = true]
    /// Labyrinth Zone - Act 2
    labyrinth_2: bool,
    #[default = true]
    /// Labyrinth Zone - Act 3
    labyrinth_3: bool,
    #[default = true]
    /// Star Light Zone - Act 1
    star_light_1: bool,
    #[default = true]
    /// Star Light Zone - Act 2
    star_light_2: bool,
    #[default = true]
    /// Star Light Zone - Act 3
    star_light_3: bool,
    #[default = true]
    /// Scrap Brain Zone - Act 1
    scrap_brain_1: bool,
    #[default = true]
    /// Scrap Brain Zone - Act 2
    scrap_brain_2: bool,
    #[default = true]
    /// Scrap Brain Zone - Act 3
    scrap_brain_3: bool,
    #[default = true]
    /// Final Zone
    final_zone: bool,
}

#[derive(Default)]
struct Watchers {
    state: Watcher<u8>,
    levelid: Watcher<Acts>,
    zoneselectongamecomplete: Watcher<u8>,
    zoneindicator: Watcher<ZoneIndicator>,
}

struct Addresses {
    state: Address,
    levelid: Address,
    zoneselectongamecomplete: Address,
    zoneindicator: Address,
    game_version: GameVersion,
}

impl Addresses {
    async fn init(process: &Process) -> Self {
        let main_module_base = retry(|| {
            PROCESS_NAMES
                .iter()
                .find_map(|&name| process.get_module_address(name).ok())
        })
        .await;
        let main_module_size =
            retry(|| pe::read_size_of_image(process, main_module_base)).await as u64;
        let main_module = (main_module_base, main_module_size);

        let is_64_bit = retry(|| pe::MachineType::read(process, main_module_base)).await
            == pe::MachineType::X86_64;

        let game_version = match is_64_bit {
            true => GameVersion::Below1_5_0,
            false => {
                if main_module_size < 0x57F4000 {
                    GameVersion::Below1_5_0
                } else {
                    GameVersion::V1_5_0OrHigher
                }
            }
        };

        let pointer_path = |ptr, lea, offset1, offset2, offset3, absolute| async move {
            match is_64_bit {
                true => match offset1 {
                    0 => lea + offset3,
                    _ => {
                        let temp_offset = retry(|| process.read::<u32>(ptr + offset1).ok()).await;
                        let temp_offset_2 = main_module_base + temp_offset + offset2;

                        match absolute {
                            true => {
                                main_module_base
                                    + retry(|| process.read::<u32>(temp_offset_2)).await
                                    + offset3
                            }
                            false => {
                                temp_offset_2
                                    + 0x4
                                    + retry(|| process.read::<u32>(temp_offset_2)).await
                                    + offset3
                            }
                        }
                    }
                },
                false => {
                    let result: Address = retry(|| {
                        process.read_pointer_path32::<Address32>(ptr, &[offset1, offset2])
                    })
                    .await
                    .into();
                    result + offset3
                }
            }
        };

        match game_version {
            GameVersion::Below1_5_0 => match is_64_bit {
                true => {
                    const SIG_64: Signature<15> =
                        Signature::new("81 F9 ???????? 0F 87 ???????? 41 8B 8C");
                    let ptr = retry(|| SIG_64.scan_process_range(process, main_module)).await + 16;
                    let ptr = main_module_base + retry(|| process.read::<u32>(ptr)).await;

                    const SIG_64_2: Signature<11> = Signature::new("48 8D 05 ???????? 49 63 F8 4C");
                    let lea = retry(|| SIG_64_2.scan_process_range(process, main_module)).await + 3;
                    let lea = lea + 0x4 + retry(|| process.read::<u32>(lea)).await;

                    let state = pointer_path(ptr, lea, 0, 0, 0x9EC, false).await;
                    let levelid = pointer_path(ptr, lea, 0x4 * 123, 2, 0, false).await;
                    let zoneselectongamecomplete = Address::NULL;

                    const SIG_64_3: Signature<15> =
                        Signature::new("C6 05 ???????? ?? E9 ???????? 48 8D 0D");
                    let ptr = retry(|| SIG_64_3.scan_process_range(process, main_module)).await + 2;
                    let zoneindicator = ptr + 0x5 + retry(|| process.read::<u32>(ptr)).await;

                    Self {
                        state,
                        levelid,
                        zoneselectongamecomplete,
                        zoneindicator,
                        game_version,
                    }
                }
                false => {
                    const SIG32: Signature<19> =
                        Signature::new("3D ???????? 0F 87 ???????? FF 24 85 ???????? A1");
                    let ptr = retry(|| SIG32.scan_process_range(process, main_module)).await + 14;
                    let ptr: Address = retry(|| process.read::<Address32>(ptr)).await.into();

                    let lea = Address::NULL;

                    let state = pointer_path(ptr, lea, 0x4 * 73, 8, 0x9D8, true).await;
                    let levelid = pointer_path(ptr, lea, 0x4 * 123, 1, 0, true).await;
                    let zoneselectongamecomplete = Address::NULL;

                    const SIG32_2: Signature<7> = Signature::new("69 F8 ???????? B8");
                    let ptr = retry(|| SIG32_2.scan_process_range(process, main_module)).await + 7;
                    let zoneindicator = retry(|| process.read::<Address32>(ptr)).await.into();

                    Self {
                        state,
                        levelid,
                        zoneselectongamecomplete,
                        zoneindicator,
                        game_version,
                    }
                }
            },
            GameVersion::V1_5_0OrHigher => {
                const SIG32: Signature<19> =
                    Signature::new("3D ???????? 0F 87 ???????? FF 24 85 ???????? A1");
                let ptr = retry(|| SIG32.scan_process_range(process, main_module)).await + 14;
                let ptr: Address = retry(|| process.read::<Address32>(ptr)).await.into();
                let lea = Address::NULL;

                let state = pointer_path(ptr, lea, 0x4 * 30, 8, 0x9D8, true).await;
                let levelid = pointer_path(ptr, lea, 0x4 * 123, 1, 0, true).await;
                let zoneselectongamecomplete = pointer_path(ptr, lea, 0x4 * 18, 3, 4, true).await;

                const SIG32_2: Signature<7> = Signature::new("69 F8 ???????? B8");
                let ptr = retry(|| SIG32_2.scan_process_range(process, main_module)).await + 7;
                let zoneindicator = retry(|| process.read::<Address32>(ptr)).await.into();

                Self {
                    state,
                    levelid,
                    zoneselectongamecomplete,
                    zoneindicator,
                    game_version,
                }
            }
        }
    }
}

fn update_loop(game: &Process, addresses: &Addresses, watchers: &mut Watchers) {
    watchers
        .state
        .update_infallible(game.read(addresses.state).unwrap_or_default());
    watchers
        .zoneselectongamecomplete
        .update_infallible(match addresses.game_version {
            GameVersion::V1_5_0OrHigher => game
                .read(addresses.zoneselectongamecomplete)
                .unwrap_or_default(),
            _ => 0,
        });

    let zone =
        watchers
            .zoneindicator
            .update_infallible(match game.read::<u32>(addresses.zoneindicator) {
                Ok(0x6E69614D) => ZoneIndicator::MainMenu,
                Ok(0x656E6F5A) => ZoneIndicator::Zones,
                Ok(0x69646E45) => ZoneIndicator::Ending,
                Ok(0x65766153) => ZoneIndicator::SaveSelect,
                _ => ZoneIndicator::Default,
            });

    watchers.levelid.update_infallible(match zone.current {
        ZoneIndicator::Ending => Acts::Default,
        ZoneIndicator::Zones => match game.read::<u8>(addresses.levelid) {
            Ok(0) => Acts::GreenHill1,
            Ok(1) => Acts::GreenHill2,
            Ok(2) => Acts::GreenHill3,
            Ok(3) => Acts::Marble1,
            Ok(4) => Acts::Marble2,
            Ok(5) => Acts::Marble3,
            Ok(6) => Acts::SpringYard1,
            Ok(7) => Acts::SpringYard2,
            Ok(8) => Acts::SpringYard3,
            Ok(9) => Acts::Labyrinth1,
            Ok(10) => Acts::Labyrinth2,
            Ok(11) => Acts::Labyrinth3,
            Ok(12) => Acts::StarLight1,
            Ok(13) => Acts::StarLight2,
            Ok(14) => Acts::StarLight3,
            Ok(15) => Acts::ScrapBrain1,
            Ok(16) => Acts::ScrapBrain2,
            Ok(17) => Acts::ScrapBrain3,
            Ok(18) => Acts::FinalZone,
            _ => Acts::Default,
        },
        _ => match &watchers.levelid.pair {
            Some(x) => x.current,
            _ => Acts::Default,
        },
    });
}

fn start(watchers: &Watchers, settings: &Settings, addresses: &Addresses) -> bool {
    let Some(state) = &watchers.state.pair else {
        return false;
    };
    let Some(zoneselectongamecomplete) = &watchers.zoneselectongamecomplete.pair else {
        return false;
    };
    let Some(zoneindicator) = &watchers.zoneindicator.pair else {
        return false;
    };

    if addresses.game_version == GameVersion::Below1_5_0 {
        let run_started_save_file = state.changed()
            && state.current == 2
            && zoneindicator.current == ZoneIndicator::SaveSelect;
        let run_started_no_save_file = state.old == 6 && state.current == 7;
        let run_started_ngp = state.old == 8 && state.current == 9;

        (settings.start_clean_save && (run_started_save_file || run_started_no_save_file))
            || (settings.start_new_game_plus && run_started_ngp)
    } else {
        let run_started_save_file = state.old == 3
            && state.current == 7
            && zoneindicator.current == ZoneIndicator::SaveSelect;
        let run_started_no_save_file = state.old == 10 && state.current == 11;
        let run_started_ngp =
            state.old == 2 && state.current == 6 && zoneselectongamecomplete.current == 0;

        (settings.start_clean_save && (run_started_save_file || run_started_no_save_file))
            || (settings.start_new_game_plus && run_started_ngp)
    }
}

fn split(watchers: &Watchers, settings: &Settings) -> bool {
    watchers.levelid.pair.is_some_and(|levelid| match levelid.current {
        Acts::GreenHill2 => settings.green_hill_1 && levelid.old == Acts::GreenHill1,
        Acts::GreenHill3 => settings.green_hill_2 && levelid.old == Acts::GreenHill2,
        Acts::Marble1 => settings.green_hill_3 && levelid.old == Acts::GreenHill3,
        Acts::Marble2 => settings.marble_1 && levelid.old == Acts::Marble1,
        Acts::Marble3 => settings.marble_2 && levelid.old == Acts::Marble2,
        Acts::SpringYard1 => settings.marble_3 && levelid.old == Acts::Marble3,
        Acts::SpringYard2 => settings.spring_yard_1 && levelid.old == Acts::SpringYard1,
        Acts::SpringYard3 => settings.spring_yard_2 && levelid.old == Acts::SpringYard2,
        Acts::Labyrinth1 => settings.spring_yard_3 && levelid.old == Acts::SpringYard3,
        Acts::Labyrinth2 => settings.labyrinth_1 && levelid.old == Acts::Labyrinth1,
        Acts::Labyrinth3 => settings.labyrinth_2 && levelid.old == Acts::Labyrinth2,
        Acts::StarLight1 => settings.labyrinth_3 && levelid.old == Acts::Labyrinth3,
        Acts::StarLight2 => settings.star_light_1 && levelid.old == Acts::StarLight1,
        Acts::StarLight3 => settings.star_light_2 && levelid.old == Acts::StarLight2,
        Acts::ScrapBrain1 => settings.star_light_3 && levelid.old == Acts::StarLight3,
        Acts::ScrapBrain2 => settings.scrap_brain_1 && levelid.old == Acts::ScrapBrain1,
        Acts::ScrapBrain3 => settings.scrap_brain_2 && levelid.old == Acts::ScrapBrain2,
        Acts::FinalZone => settings.scrap_brain_3 && levelid.old == Acts::ScrapBrain3,
        Acts::Default => settings.final_zone && levelid.old != levelid.current,
        _ => false,
    })
}

fn reset(watchers: &Watchers, settings: &Settings, addresses: &Addresses) -> bool {
    settings.reset
        && match addresses.game_version {
            GameVersion::Below1_5_0 => {
                watchers
                    .state
                    .pair
                    .is_some_and(|val| val.changed_from_to(&200, &201))
                    && watchers
                        .zoneindicator
                        .pair
                        .is_some_and(|val| val.current == ZoneIndicator::SaveSelect)
            }
            _ => {
                watchers
                    .state
                    .pair
                    .is_some_and(|val| val.changed_from_to(&13, &14))
                    && watchers
                        .zoneindicator
                        .pair
                        .is_some_and(|val| val.current == ZoneIndicator::SaveSelect)
            }
        }
}

#[derive(Clone, Copy, PartialEq)]
enum ZoneIndicator {
    MainMenu,
    Zones,
    Ending,
    SaveSelect,
    Default,
}

#[derive(Clone, Copy, PartialEq)]
enum Acts {
    GreenHill1,
    GreenHill2,
    GreenHill3,
    Marble1,
    Marble2,
    Marble3,
    SpringYard1,
    SpringYard2,
    SpringYard3,
    Labyrinth1,
    Labyrinth2,
    Labyrinth3,
    StarLight1,
    StarLight2,
    StarLight3,
    ScrapBrain1,
    ScrapBrain2,
    ScrapBrain3,
    FinalZone,
    Default,
}

#[derive(Clone, Copy, PartialEq)]
enum GameVersion {
    Below1_5_0,
    V1_5_0OrHigher,
}
