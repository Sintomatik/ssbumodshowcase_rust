// ═══════════════════════════════════════════════════════════════════════════════
// MECHA SONIC MOVESET — Super Smash Bros. Ultimate Character Mod (Examples Showcase)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Author:           Sintomatikoa
// Platform:         Nintendo Switch (ARM64 / aarch64-skyline-switch)
// Language:         Rust (no_std, FFI with C game engine)
// Framework:        Skyline (runtime code injection) + Smashline (fighter hooks)
// Techniques Used:  Reverse engineering (Ghidra), finite state machines, physics
//                   simulation, real-time mesh manipulation, low-level kinetic
//                   energy control, animation scripting, and memory-mapped
//                   flag/work-variable systems.
//
// This file contains curated excerpts from the full Mecha Sonic moveset mod that
// demonstrate a few examples of SSBU moveset modding.
// The code hooks into the live game process at runtime, replacing and
// extending Samus's moveset to create an entirely new character.
//
// NOTE: This file is for documentation purposes and is not compiled.
// ═══════════════════════════════════════════════════════════════════════════════


// ─────────────────────────────────────────────────────────────────────────────
// 1. TYPE SYSTEM & STATE MACHINE — Form Management
// ─────────────────────────────────────────────────────────────────────────────
//
// Mecha Sonic has four visual forms that must be tracked and rendered
// correctly across all game states. This is modelled as a product of two
// independent boolean flags, mapped to a sum type (enum) via pattern matching.

/// The four visual forms Mecha Sonic can be in.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MechaFormState {
    Normal,     // Base body mesh
    Super,      // Powered-up body (unlocked via Final Smash)
    Ball,       // Morph ball (during down special)
    SuperBall,  // Powered-up morph ball
}

/// Eye glow states — toggled contextually by damage, taunts, and sleep.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MechaEyeState {
    Off,     // Powered down (taunt, sleep, KO screen)
    On,      // Normal operation
    Glitch,  // Damage flash / dizzy
}

/// Derives the current form from two independent boolean flags stored in the
/// game's per-fighter work memory.  Demonstrates exhaustive pattern matching.
pub unsafe fn get_current_form_state(
    boma: *mut smash::app::BattleObjectModuleAccessor,
) -> MechaFormState {
    let is_super = WorkModule::is_flag(boma, FLAG_SUPER_FORM);
    let in_ball  = WorkModule::is_flag(boma, FLAG_IN_BALL_FORM);

    match (is_super, in_ball) {
        (false, false) => MechaFormState::Normal,
        (true,  false) => MechaFormState::Super,
        (false, true)  => MechaFormState::Ball,
        (true,  true)  => MechaFormState::SuperBall,
    }
}

/// Applies mesh visibility to the 3D model.  Each form shows/hides a
/// different combination of sub-meshes on the character's skeleton.
/// Ball forms also disable eye meshes since they aren't visible.
pub unsafe fn apply_mecha_form_visibility(
    boma: *mut smash::app::BattleObjectModuleAccessor,
    form_state: MechaFormState,
) {
    if !is_mecha(boma) { return; }

    match form_state {
        MechaFormState::Normal => {
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_body"), true);
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_superbody"), false);
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_ball"), false);
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_superball"), false);
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_permabody"), true);
        },
        MechaFormState::Super => {
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_body"), false);
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_superbody"), true);
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_ball"), false);
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_superball"), false);
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_permabody"), true);
        },
        MechaFormState::Ball | MechaFormState::SuperBall => {
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_body"), false);
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_superbody"), false);
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_permabody"), false);
            // Hide all eye meshes — not visible inside ball
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_eyeoff"), false);
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_eyeon"), false);
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_eyeglitch"), false);
            // Show the correct ball variant
            let is_super = form_state == MechaFormState::SuperBall;
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_ball"), !is_super);
            ModelModule::set_mesh_visibility(boma, Hash40::new("samus_superball"), is_super);
        },
    }

    // Eye visibility is only relevant for non-ball forms
    if form_state != MechaFormState::Ball && form_state != MechaFormState::SuperBall {
        update_mecha_eye_visibility(boma);
    }
}


// ─────────────────────────────────────────────────────────────────────────────
// 2. PER-FRAME EYE STATE MACHINE — Animation-Driven Visual Feedback
// ─────────────────────────────────────────────────────────────────────────────
//
// Runs every frame (60 Hz).  Reads the current animation hash and maps it
// to the appropriate eye glow state, giving Mecha Sonic reactive expressions
// (glitching on damage, powering off on sleep/taunt).

unsafe extern "C" fn eye_animation_opff(fighter: &mut L2CAgentBase) {
    if !is_mecha(fighter.module_accessor) { return; }

    let motion_kind = MotionModule::motion_kind(fighter.module_accessor);

    let new_eye_state = match motion_kind {
        // Taunts and sleep → power down
        x if x == hash40("appeal_lw_l")     => Some(MechaEyeState::Off),
        x if x == hash40("appeal_lw_r")     => Some(MechaEyeState::Off),
        x if x == hash40("fura_sleep_start") => Some(MechaEyeState::Off),
        x if x == hash40("fura_sleep_loop")  => Some(MechaEyeState::Off),
        x if x == hash40("lose")             => Some(MechaEyeState::Off),

        // All damage / dizzy animations → glitch effect
        x if x == hash40("damage_hi_1")
           | (x == hash40("damage_hi_2"))
           | (x == hash40("damage_hi_3"))
           | (x == hash40("damage_fly_top"))
           | (x == hash40("damage_elec"))
           | (x == hash40("furafura"))       => Some(MechaEyeState::Glitch),

        // Everything else → normal "On" (only set if not already On)
        _ => {
            let current = get_eye_state(fighter.module_accessor);
            if current != MechaEyeState::On { Some(MechaEyeState::On) } else { None }
        }
    };

    if let Some(state) = new_eye_state {
        set_eye_state(fighter.module_accessor, state);
    }
}


// ─────────────────────────────────────────────────────────────────────────────
// 3. CUSTOM TELEPORT UP-SPECIAL — Full Status Script
// ─────────────────────────────────────────────────────────────────────────────
//
// Replaces Samus's Screw Attack with an instant teleport.  The player aims
// with the control stick during a short startup window; on frame 8 the
// character is warped to the target position.  After the warp, gravity is
// re-enabled and the fighter enters a fall-special state.
//
// This demonstrates the full lifecycle of a custom fighter "status":
//   Pre  → sets physics type, ground correction, logging masks
//   Init → disables all kinetic energies, zeroes velocity
//   Main → starts animation, registers the per-frame loop callback
//   Loop → reads stick input, teleports, manages post-warp physics,
//          checks ledge grabs (both directions), handles landing
//   Exit → cleans up flags, sets reduced landing lag

unsafe extern "C" fn mecha_special_hi_main_loop(
    fighter: &mut L2CFighterCommon,
) -> L2CValue {
    let frame = MotionModule::frame(fighter.module_accessor);

    // ── Ledge grab (checked every frame, both directions) ──────────────
    WorkModule::enable_transition_term(
        fighter.module_accessor,
        *FIGHTER_STATUS_TRANSITION_TERM_ID_CLIFF_CATCH,
    );
    if fighter.sub_transition_group_check_air_cliff().get_bool() {
        return 1.into();  // Engine handles the rest
    }
    // Check behind (reverse LR, test, reverse back)
    PostureModule::reverse_lr(fighter.module_accessor);
    PostureModule::update_rot_y_lr(fighter.module_accessor);
    if fighter.sub_transition_group_check_air_cliff().get_bool() {
        return 1.into();
    }
    PostureModule::reverse_lr(fighter.module_accessor);
    PostureModule::update_rot_y_lr(fighter.module_accessor);

    // ── Frame 8: read stick and teleport ───────────────────────────────
    if frame >= 8.0 && frame < 9.0
        && !WorkModule::is_flag(fighter.module_accessor, *FIGHTER_SAMUS_STATUS_SPECIAL_HI_FLAG_DISABLE_LR)
    {
        WorkModule::on_flag(
            fighter.module_accessor,
            *FIGHTER_SAMUS_STATUS_SPECIAL_HI_FLAG_DISABLE_LR,
        );

        let stick_x = ControlModule::get_stick_x(fighter.module_accessor);
        let stick_y = ControlModule::get_stick_y(fighter.module_accessor);

        let base_distance = 80.0;
        let magnitude = (stick_x * stick_x + stick_y * stick_y)
            .sqrt()
            .min(1.0)
            .max(0.5);  // Minimum 50% distance even with small input
        let distance = base_distance * magnitude;

        let (dx, dy) = if magnitude > 0.2 {
            (stick_x * distance, stick_y * distance)
        } else {
            (0.0, 0.5 * distance)  // Default: slightly upward
        };

        // Instant position warp
        let pos = *PostureModule::pos(fighter.module_accessor);
        PostureModule::set_pos(
            fighter.module_accessor,
            &Vector3f { x: pos.x + dx, y: pos.y + dy, z: pos.z },
        );

        // Kill all residual velocity after warp
        KineticModule::unable_energy(fighter.module_accessor, *FIGHTER_KINETIC_ENERGY_ID_CONTROL);
        KineticModule::unable_energy(fighter.module_accessor, *FIGHTER_KINETIC_ENERGY_ID_GRAVITY);
        KineticModule::unable_energy(fighter.module_accessor, *FIGHTER_KINETIC_ENERGY_ID_MOTION);
        // ... (velocity reset via sv_kinetic_energy omitted for brevity)
    }

    // ── Frame 19: re-enable gravity for natural fall ───────────────────
    if frame >= 19.0 && frame < 20.0 {
        KineticModule::enable_energy(
            fighter.module_accessor,
            *FIGHTER_KINETIC_ENERGY_ID_GRAVITY,
        );
        KineticModule::enable_energy(
            fighter.module_accessor,
            *FIGHTER_KINETIC_ENERGY_ID_CONTROL,
        );
    }

    // ── Landing during endlag (reduced lag) ────────────────────────────
    if frame >= 36.0 && fighter.global_table[0x16] == SITUATION_KIND_GROUND {
        WorkModule::set_float(
            fighter.module_accessor,
            34.0,  // 75% of base landing lag
            *FIGHTER_INSTANCE_WORK_ID_FLOAT_LANDING_FRAME,
        );
        fighter.change_status(
            L2CValue::I32(*FIGHTER_STATUS_KIND_LANDING_FALL_SPECIAL),
            L2CValue::Bool(false),
        );
        return 1.into();
    }

    // ── Animation ended → fall special (50% landing lag) ───────────────
    if MotionModule::is_end(fighter.module_accessor) {
        WorkModule::set_float(
            fighter.module_accessor,
            22.0,
            *FIGHTER_INSTANCE_WORK_ID_FLOAT_LANDING_FRAME,
        );
        fighter.change_status(
            L2CValue::I32(*FIGHTER_STATUS_KIND_FALL_SPECIAL),
            L2CValue::Bool(false),
        );
        return 1.into();
    }

    0.into()
}


// ─────────────────────────────────────────────────────────────────────────────
// 4. RIFLE SIDE-SPECIAL — Looping Fire with Arm Aiming & Cancel System
// ─────────────────────────────────────────────────────────────────────────────
//
// A hold-to-fire rifle that loops its shooting animation while B is held,
// tracks ammo via a per-frame timer, and allows cancellation into a wide
// set of actions (aerials, dodges, jumps, other specials).
//
// The arm is rotated in real-time to follow the control stick's Y axis
// using smoothed linear interpolation, producing fluid aim visuals.

// Ammo system — fire for 140 frames (~2.3 seconds), then force endlag.
pub const RIFLE_DEPLETION_FRAMES: f32 = 140.0;

/// Per-frame ammo tracker (runs via OPFF hook at 60 Hz).
unsafe extern "C" fn rifle_timer_opff(fighter: &mut L2CAgentBase) {
    if !is_mecha(fighter.module_accessor) { return; }

    let motion = MotionModule::motion_kind(fighter.module_accessor);
    let in_endlag = WorkModule::is_flag(
        fighter.module_accessor,
        *FIGHTER_SAMUS_STATUS_SPECIAL_S_WORK_FLAG_AIR_CONTROL,
    );
    let in_rifle = motion == hash40("special") || motion == hash40("special_air");

    if in_rifle && !in_endlag {
        let timer = WorkModule::get_float(
            fighter.module_accessor,
            WORK_ID_RIFLE_TIMER,
        ) + 1.0;
        WorkModule::set_float(fighter.module_accessor, timer, WORK_ID_RIFLE_TIMER);
        if timer >= RIFLE_DEPLETION_FRAMES {
            WorkModule::on_flag(fighter.module_accessor, FLAG_RIFLE_OUT_OF_BULLETS);
        }
    } else {
        // Reset when not firing
        WorkModule::set_float(fighter.module_accessor, 0.0, WORK_ID_RIFLE_TIMER);
        WorkModule::off_flag(fighter.module_accessor, FLAG_RIFLE_OUT_OF_BULLETS);
    }
}

/// Smoothly rotates the arm joint to follow stick Y, producing aim visuals.
/// Called every frame during rifle status (ground and air variants).
unsafe fn apply_arm_aim(fighter: &mut L2CFighterCommon) {
    let stick_y   = ControlModule::get_stick_y(fighter.module_accessor);
    let prev      = WorkModule::get_float(fighter.module_accessor, WORK_ID_ARM_ANGLE);
    let target    = stick_y * 35.0;                   // ±35° max tilt
    let smoothed  = prev + (target - prev) * 0.15;    // Lerp, 15% per frame

    ModelModule::set_joint_rotate(
        fighter.module_accessor,
        Hash40::new("armr"),
        &Vector3f { x: 0.0, y: 0.0, z: -smoothed },
        RotateCompose::After,
        RotateOrder::XYZ,
    );
    WorkModule::set_float(fighter.module_accessor, smoothed, WORK_ID_ARM_ANGLE);
}

/// Air rifle cancel system — demonstrates complex input-driven branching.
/// While airborne, the rifle can be cancelled into any aerial attack,
/// air dodge, double jump, or special move, giving the player deep
/// mix-up options from a single move.
unsafe fn check_air_rifle_cancels(fighter: &mut L2CFighterCommon) -> bool {
    // A button / C-stick → directional aerial attack
    if ControlModule::check_button_on(fighter.module_accessor, *CONTROL_PAD_BUTTON_ATTACK) {
        AttackModule::clear_all(fighter.module_accessor);
        let stick_y = ControlModule::get_stick_y(fighter.module_accessor);
        let stick_x = ControlModule::get_stick_x(fighter.module_accessor);
        let lr      = PostureModule::lr(fighter.module_accessor);

        let aerial_hash = if stick_y > 0.5 {
            hash40("attack_air_hi")
        } else if stick_y < -0.5 {
            hash40("attack_air_lw")
        } else if stick_x * lr > 0.5 {
            hash40("attack_air_f")
        } else if stick_x * lr < -0.5 {
            hash40("attack_air_b")
        } else {
            hash40("attack_air_n")
        };

        WorkModule::set_int64(
            fighter.module_accessor,
            aerial_hash as i64,
            *FIGHTER_STATUS_ATTACK_AIR_WORK_INT_MOTION_KIND,
        );
        fighter.change_status(FIGHTER_STATUS_KIND_ATTACK_AIR.into(), true.into());
        return true;
    }

    // Shield → air dodge
    if ControlModule::check_button_on(fighter.module_accessor, *CONTROL_PAD_BUTTON_GUARD) {
        AttackModule::clear_all(fighter.module_accessor);
        fighter.change_status(FIGHTER_STATUS_KIND_ESCAPE_AIR.into(), true.into());
        return true;
    }

    // Jump (if jumps remaining) → double jump
    if ControlModule::check_button_trigger(fighter.module_accessor, *CONTROL_PAD_BUTTON_JUMP) {
        let count = WorkModule::get_int(fighter.module_accessor, *FIGHTER_INSTANCE_WORK_ID_INT_JUMP_COUNT);
        let max   = WorkModule::get_int(fighter.module_accessor, *FIGHTER_INSTANCE_WORK_ID_INT_JUMP_COUNT_MAX);
        if count < max {
            AttackModule::clear_all(fighter.module_accessor);
            fighter.change_status(FIGHTER_STATUS_KIND_JUMP_AERIAL.into(), true.into());
            return true;
        }
    }

    false
}


// ─────────────────────────────────────────────────────────────────────────────
// 5. DOWN SPECIAL — Ball Form with Phase-Based Physics
// ─────────────────────────────────────────────────────────────────────────────
//
// Mecha Sonic curls into a ball and rolls forward, then uncurls.  The move
// is split into timed phases, each with distinct physics and visual states.
// The mesh swaps to a ball model during the active window and restores on
// exit — including interrupted exits (hit, cancel, ledge grab, landing).

unsafe extern "C" fn mecha_special_lw_main_loop(
    fighter: &mut L2CFighterCommon,
) -> L2CValue {
    let frame = MotionModule::frame(fighter.module_accessor);

    //  Phase 1: Startup (0–27)   — curl into ball at frame 7
    //  Phase 2: Active  (27–37)  — rolling with physics
    //  Phase 3: Recovery (37–45) — uncurl, disable movement
    //  Phase 4: Late    (45–93)  — stop all velocity, wait for end

    if frame < 27.0 {
        // Frame 7: swap to ball mesh
        if frame >= 7.0 && frame < 8.0 {
            set_ball_form_state(fighter.module_accessor, true);
            let form = if get_super_form_state(fighter.module_accessor) {
                MechaFormState::SuperBall
            } else {
                MechaFormState::Ball
            };
            apply_mecha_form_visibility(fighter.module_accessor, form);
        }
    } else if frame < 37.0 {
        // Active rolling — physics applied via separate function
    } else if frame >= 37.0 && frame < 38.0 {
        // Recovery: disable movement, restore normal mesh
        WorkModule::off_flag(
            fighter.module_accessor,
            *FIGHTER_SAMUS_STATUS_SPECIAL_LW_FLAG_MV,
        );
        set_ball_form_state(fighter.module_accessor, false);
        let form = if get_super_form_state(fighter.module_accessor) {
            MechaFormState::Super
        } else {
            MechaFormState::Normal
        };
        apply_mecha_form_visibility(fighter.module_accessor, form);
    } else if frame >= 78.0 && frame < 79.0 {
        // Late phase: kill all residual velocity
        KineticModule::change_kinetic(
            fighter.module_accessor,
            *FIGHTER_KINETIC_TYPE_GROUND_STOP,
        );
        let zero = Vector3f { x: 0.0, y: 0.0, z: 0.0 };
        KineticModule::mul_speed(fighter.module_accessor, &zero, *FIGHTER_KINETIC_ENERGY_ID_STOP);
        KineticModule::mul_speed(fighter.module_accessor, &zero, *FIGHTER_KINETIC_ENERGY_ID_MOTION);
    }

    // Check for cancel window
    if check_exit_conditions(fighter) {
        return transition_to_appropriate_status(fighter);
    }

    // Natural end
    if frame >= 93.0 || MotionModule::is_end(fighter.module_accessor) {
        fighter.change_status(FIGHTER_STATUS_KIND_WAIT.into(), false.into());
        return 1.into();
    }

    0.into()
}


// ─────────────────────────────────────────────────────────────────────────────
// 6. CONTINUOUS MESH ENFORCEMENT — Defensive Per-Frame Consistency Check
// ─────────────────────────────────────────────────────────────────────────────
//
// The game engine can reset mesh visibility during status transitions,
// hitlag, and special cinematics.  This per-frame hook acts as a safety
// net: it re-derives the correct visual state from authoritative flags
// and status/motion checks, then enforces it every frame.
//
// It uses a dual-source verification strategy:
//   1. Status kind (e.g. FIGHTER_STATUS_KIND_SPECIAL_LW)
//   2. Motion hash (e.g. hash40("special_lw"))
// If either source says "down special", AND the frame is in the ball
// window (7–37), ball meshes are shown.  A whitelist of "definitely
// normal" statuses (WAIT, FALL, LANDING, etc.) overrides everything.

unsafe extern "C" fn mesh_force(fighter: &mut L2CAgentBase) {
    if !is_mecha(fighter.module_accessor) { return; }

    let status = StatusModule::status_kind(fighter.module_accessor);
    if status == *FIGHTER_STATUS_KIND_WIN || status == *FIGHTER_STATUS_KIND_LOSE {
        return;  // Don't interfere with victory/defeat screens
    }

    let motion   = MotionModule::motion_kind(fighter.module_accessor);
    let is_super = get_super_form_state(fighter.module_accessor);

    // Whitelist: these statuses can NEVER be ball form
    let definitely_normal =
           status == *FIGHTER_STATUS_KIND_WAIT
        || status == *FIGHTER_STATUS_KIND_FALL
        || status == *FIGHTER_STATUS_KIND_FALL_AERIAL
        || status == *FIGHTER_STATUS_KIND_LANDING
        || motion == hash40("wait")
        || motion == hash40("fall");

    let should_be_ball = if definitely_normal {
        false
    } else {
        let in_down_special =
               status == *FIGHTER_STATUS_KIND_SPECIAL_LW
            || status == *FIGHTER_SAMUS_STATUS_KIND_SPECIAL_AIR_LW
            || motion == hash40("special_lw")
            || motion == hash40("special_air_lw");

        if in_down_special {
            let frame = MotionModule::frame(fighter.module_accessor);
            frame >= 7.0 && frame < 37.0
        } else {
            false
        }
    };

    // Sync flag with computed state
    if should_be_ball != get_ball_form_state(fighter.module_accessor) {
        set_ball_form_state(fighter.module_accessor, should_be_ball);
    }

    // Apply the correct meshes
    if should_be_ball {
        ModelModule::set_mesh_visibility(fighter.module_accessor, Hash40::new("samus_ball"), true);
        ModelModule::set_mesh_visibility(fighter.module_accessor, Hash40::new("samus_body"), false);
        ModelModule::set_mesh_visibility(fighter.module_accessor, Hash40::new("samus_superbody"), false);
        ModelModule::set_mesh_visibility(fighter.module_accessor, Hash40::new("samus_permabody"), false);
    } else {
        ModelModule::set_mesh_visibility(fighter.module_accessor, Hash40::new("samus_ball"), false);
        if is_super {
            ModelModule::set_mesh_visibility(fighter.module_accessor, Hash40::new("samus_superbody"), true);
            ModelModule::set_mesh_visibility(fighter.module_accessor, Hash40::new("samus_body"), false);
        } else {
            ModelModule::set_mesh_visibility(fighter.module_accessor, Hash40::new("samus_body"), true);
            ModelModule::set_mesh_visibility(fighter.module_accessor, Hash40::new("samus_superbody"), false);
        }
        ModelModule::set_mesh_visibility(fighter.module_accessor, Hash40::new("samus_permabody"), true);
        update_mecha_eye_visibility(fighter.module_accessor);
    }
}


// ─────────────────────────────────────────────────────────────────────────────
// 7. FINAL SMASH → SUPER FORM TRIGGER — Cross-Status Event Propagation
// ─────────────────────────────────────────────────────────────────────────────
//
// When Mecha Sonic lands a Final Smash, it permanently upgrades to Super
// form for the rest of that stock.  The challenge: the Final Smash spans
// multiple internal statuses (START → LOOP → END), and mesh changes made
// during the cinematic can be overwritten when the fighter returns to
// normal gameplay.
//
// Solution: a "trigger" flag is set on Final Smash start.  A per-frame
// check watches for the flag and waits until the fighter has LEFT all
// Final Smash statuses before applying the permanent upgrade — ensuring
// the mesh change isn't clobbered by the cinematic cleanup.

unsafe extern "C" fn final_smash_super_form_check(fighter: &mut L2CAgentBase) {
    if !is_mecha(fighter.module_accessor) { return; }

    let status = StatusModule::status_kind(fighter.module_accessor);

    // Set trigger on Final Smash start (ground or air variant)
    if status == *FIGHTER_SAMUS_STATUS_KIND_FINAL_START_A
    || status == *FIGHTER_SAMUS_STATUS_KIND_FINAL_START_G
    {
        WorkModule::set_flag(fighter.module_accessor, true, FLAG_FINAL_SMASH_TRIGGERED);
        if !get_super_form_state(fighter.module_accessor) {
            set_super_form_state(fighter.module_accessor, true);
            apply_mecha_form_visibility(fighter.module_accessor, MechaFormState::Super);
        }
    }

    // Wait until we've LEFT all Final Smash statuses, then apply permanently
    if WorkModule::is_flag(fighter.module_accessor, FLAG_FINAL_SMASH_TRIGGERED) {
        let still_in_final =
               status == *FIGHTER_SAMUS_STATUS_KIND_FINAL_START_A
            || status == *FIGHTER_SAMUS_STATUS_KIND_FINAL_START_G
            || status == *FIGHTER_SAMUS_STATUS_KIND_FINAL_LOOP_A
            || status == *FIGHTER_SAMUS_STATUS_KIND_FINAL_LOOP_G
            || status == *FIGHTER_SAMUS_STATUS_KIND_FINAL_END_A
            || status == *FIGHTER_SAMUS_STATUS_KIND_FINAL_END_G;

        if !still_in_final {
            WorkModule::set_flag(fighter.module_accessor, false, FLAG_FINAL_SMASH_TRIGGERED);
            set_super_form_state(fighter.module_accessor, true);
            let form = if get_ball_form_state(fighter.module_accessor) {
                MechaFormState::SuperBall
            } else {
                MechaFormState::Super
            };
            apply_mecha_form_visibility(fighter.module_accessor, form);
        }
    }
}
