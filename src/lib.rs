use std::path::{PathBuf, Path};

#[derive(Clone, Debug)]
pub struct Combo {
    pub path: PathBuf,
    pub start: usize,
    pub end: usize,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TargetPathError {
    PathNotFound,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub lead_in: usize,
    pub lead_out: usize,

    /// Must be between 0 and 1
    pub strictness: f32,

    pub player_character: Option<slp_parser::Character>,
    pub player_code: Option<String>,
    pub player_name: Option<String>,
    pub opponent_character: Option<slp_parser::Character>,
    pub opponent_code: Option<String>,
    pub opponent_name: Option<String>,
}

impl Config {
    pub const DEFAULT: Self = Config {
        lead_in: 30,
        lead_out: 0,
        strictness: 0.5,

        player_character: None,
        player_code: None,
        player_name: None,
        opponent_character: None,
        opponent_code: None,
        opponent_name: None,
    };
}

/// Given a list of frames, tries to find a good place to start a combo which lasts till the end of the list.
///
/// 0 is least strict, 1 is most strict.
fn combo_start(
    // must be same len
    atk_frame: &[slp_parser::Frame],
    def_frame: &[slp_parser::Frame],

    strictness: f32, 
) -> Option<usize> {
    let max_defender_consecutive_actionable = (35.0 - 10.0 * strictness).round() as usize;
    let max_attacker_total_hitstun          = (65.0 - 10.0 * strictness).round() as usize;
    let max_attacker_consecutive_grab_count = ( 6.0 -  4.0 * strictness).round() as usize;
    let min_attacker_attack_actions         = ( 3.0 +  6.0 * strictness).round() as usize;
    let min_defender_total_damage           = (20.0 + 40.0 * strictness).round();

    use slp_parser::{ActionState, StandardActionState, BroadState, StandardBroadState};

    // first pass ----------
    // determines potential start of combo

    let mut last_hit_end = None;
    for f in (0..atk_frame.len()).rev() {
        let defender_state = def_frame[f].state.broad_state();
        match defender_state {
            BroadState::Standard(StandardBroadState::Hitstun | StandardBroadState::Ground | StandardBroadState::Attack) => {
                last_hit_end = Some(f);
                break;
            }
            _ => (),
        }
    }
    let last_hit_end = last_hit_end?;

    let mut defender_consecutive_actionable = max_defender_consecutive_actionable;
    let mut attacker_total_hitstun = max_attacker_total_hitstun;
    let mut first_hit = None;

    for f in (0..last_hit_end).rev() {
        let attacker_state = atk_frame[f].state.broad_state();
        let defender_state = def_frame[f].state.broad_state();

        match defender_state {
            BroadState::Standard(StandardBroadState::Hitstun) => first_hit = Some(f),
            _ => (),
        }
        
        if matches!(
            defender_state, 
            BroadState::Standard(StandardBroadState::Attack | StandardBroadState::GenericInactionable) | BroadState::Special(_) 
        ) || matches!(
            defender_state, 
            BroadState::Standard(s) if s.is_actionable()
        ) {
            defender_consecutive_actionable -= 1;
        } else {
            defender_consecutive_actionable = max_defender_consecutive_actionable;
        }

        match attacker_state {
            BroadState::Standard(StandardBroadState::Hitstun) => attacker_total_hitstun -= 1,
            _ => (),
        }

        if attacker_total_hitstun == 0 { break }
        if defender_consecutive_actionable == 0 { break }
    }

    // pruning passes ---------
    // various more complicated checks

    if let Some(first) = first_hit {
        // defender
        let damage_dealt = def_frame.last().unwrap().percent - def_frame[first-1].percent;
        if damage_dealt < min_defender_total_damage { return None; }

        // attacker
        let mut attacker_consecutive_grabs = max_attacker_consecutive_grab_count;
        let mut attacker_attacks = 0;
        for f in atk_frame[first..last_hit_end].iter() {
            // advance grab counter
            if (
                f.state == ActionState::Standard(StandardActionState::Catch)
                || f.state == ActionState::Standard(StandardActionState::CatchDash)
            ) && f.anim_frame == 0.0 {
                attacker_consecutive_grabs -= 1;
            }

            // reset grab counter on attack or special, and advance attack counter
            if matches!(
                f.state.broad_state(), 
                BroadState::Standard(StandardBroadState::Attack) | BroadState::Special(_) 
            ) {
                attacker_consecutive_grabs = max_attacker_consecutive_grab_count;

                if f.anim_frame == 1.0 {
                    attacker_attacks += 1;
                }
            }

            if attacker_consecutive_grabs == 0 { return None; }
        }

        if attacker_attacks < min_attacker_attack_actions { return None; }
    }

    first_hit
}

/// If path is invalid or cannot be parsed, immediately returns zero.
fn combos(
    config: &Config,
    path: &Path,
    combos: &std::sync::Mutex<&mut Vec<Combo>>,
) -> usize {
    fn inner<'a>(
        atk_frame: &[slp_parser::Frame],
        def_frame: &[slp_parser::Frame],

        config: &Config,
        path: &Path,
        combos: &std::sync::Mutex<&mut Vec<Combo>>,
        found: &mut usize,
    ) {
        let frame_count = atk_frame.len();

        let mut f = 0;
        while f < frame_count {
            if def_frame[f].state.broad_state() != slp_parser::StandardBroadState::Dead.into() { 
                f += 1;
                continue;
            }

            loop {
                // second character check to make sure it's not transformed sheik/zelda
                if config.player_character.is_some_and(|c| c != atk_frame[f].character) { break; }
                if config.opponent_character.is_some_and(|c| c != def_frame[f].character) { break; }

                if let Some(kill_combo_start) = combo_start(
                    &atk_frame[..f],
                    &def_frame[..f],
                    config.strictness,
                ) {
                    let start = kill_combo_start.saturating_sub(config.lead_in);
                    combos.lock().unwrap().push(Combo {
                        path: path.to_path_buf(), 
                        start,
                        end: (f+config.lead_out).min(frame_count),
                    });
                    *found += 1
                }

                break;
            }

            f += 1;
            while f < frame_count && def_frame[f].state.broad_state() == slp_parser::StandardBroadState::Dead.into() { f += 1; }
        }
    }

    fn passes(
        config: &Config,
        p_char: slp_parser::Character,
        p_code: &str,
        p_name: &str,
        o_char: slp_parser::Character,
        o_code: &str,
        o_name: &str,
    ) -> bool {
        if config.player_character  .is_some_and(|c| c != p_char) { return false }
        if config.opponent_character.is_some_and(|c| c != o_char) { return false }
        if config.player_name       .as_ref().is_some_and(|c| !p_name.contains(c)) { return false }
        if config.opponent_name     .as_ref().is_some_and(|c| !o_name.contains(c)) { return false }
        if config.player_code       .as_ref().is_some_and(|c| !p_code.contains(c)) { return false }
        if config.opponent_code     .as_ref().is_some_and(|c| !o_code.contains(c)) { return false }

        true
    }

    let info = match slp_parser::read_info(path) {
        Ok(i) => i,
        Err(_) => return 0,
    };

    let (low_port, high_port) = match info.low_high_ports() {
        Some(p) => p,
        None => return 0,
    };

    let mut buf = String::with_capacity(128);

    let p1_char = info.starting_character_colours[low_port].unwrap().character();
    let p2_char = info.starting_character_colours[high_port].unwrap().character();

    slp_parser::decode_shift_jis(&info.names[low_port], &mut buf).unwrap();
    let p1_name_end = buf.len();
    slp_parser::decode_shift_jis(&info.names[high_port], &mut buf).unwrap();
    let p2_name_end = buf.len();
    slp_parser::decode_shift_jis(&info.connect_codes[low_port], &mut buf).unwrap();
    let p1_code_end = buf.len();
    slp_parser::decode_shift_jis(&info.connect_codes[high_port], &mut buf).unwrap();
    let p2_code_end = buf.len();
    
    let p1_name = &buf[0..p1_name_end];
    let p2_name = &buf[p1_name_end..p2_name_end];
    let p1_code = &buf[p2_name_end..p1_code_end];
    let p2_code = &buf[p1_code_end..p2_code_end];

    let p1_passes = passes(config, p1_char, p1_code, p1_name, p2_char, p2_code, p2_name);
    let p2_passes = passes(config, p2_char, p2_code, p2_name, p1_char, p1_code, p1_name);
    
    let mut found = 0;

    if p1_passes | p2_passes {
        let (game, _) = match slp_parser::read_game(path) {
            Ok(g) => g,
            Err(_) => return 0,
        };

        let f1 = game.frames[low_port].as_ref().unwrap();
        let f2 = game.frames[high_port].as_ref().unwrap();

        if p1_passes {
            inner(f1, f2, config, path, combos, &mut found)
        }

        if p2_passes {
            inner(f2, f1, config, path, combos, &mut found)
        }
    }

    found
}

pub fn target_path(
    config: &Config,
    path: &Path,
    sender: Option<std::sync::mpsc::Sender<usize>>,
) -> Result<Vec<Combo>, TargetPathError> {
    if !matches!(path.try_exists(), Ok(true)) { return Err(TargetPathError::PathNotFound) }
    
    let mut targets = Vec::new();
    get_targets(&mut targets, &path);
    if let Some(ref sender) = sender { sender.send(targets.len()).expect("Sending failed"); }

    let mut combo_vec = Vec::new();

    {
        let combo_list = std::sync::Arc::new(std::sync::Mutex::new(&mut combo_vec));
        if targets.len() < 8 {
            for t in targets.iter() { 
                combos(&config, t, &combo_list);
                if let Some(ref sender) = sender { sender.send(1).expect("Sending failed"); }
            }
        } else {
            // split into 8 approximately equal slices (why is this so annoying?)
            let mut slices: [&[PathBuf]; 8] = [&[]; 8];
            let chunk = targets.len() / 8;
            let split = (chunk + 1) * (targets.len() % 8);
            for (i, c) in targets[..split].chunks(chunk+1).chain(targets[split..].chunks(chunk)).enumerate() {
                slices[i] = c;
            }
            
            let sender_ref = sender.as_ref();

            std::thread::scope(|scope| {
                for s in slices {
                    let thread_combo_list = combo_list.clone();
                    scope.spawn(move || {
                        let sender = sender_ref.clone();

                        for t in s { 
                            combos(&config, &t, &thread_combo_list);
                            if let Some(ref sender) = sender { sender.send(1).expect("Sending failed"); }
                        }
                    });
                }
            })
        }
    }
    
    // all refs dropped by now
    Ok(combo_vec)
}

fn get_targets(
    targets: &mut Vec<std::path::PathBuf>, 
    path: &std::path::Path, 
) -> Option<()> {
    for f in std::fs::read_dir(path).ok()? {
        let f = match f {
            Ok(f) => f,
            Err(_) => continue,
        };

        let path = f.path();

        if path.is_dir() { get_targets(targets, &path); }
        if !path.is_file() { continue; }
        let ex = path.extension();
        if ex == Some(std::ffi::OsStr::new("slp")) || ex == Some(std::ffi::OsStr::new("slpz")) {
            targets.push(path)
        }
    }

    Some(())
}

pub fn write_playlist(combos: &[Combo], out_json_path: &std::path::Path) -> std::io::Result<()> {
    // write json --------------------------------------
    
    let queue_json = combos.iter()
        .map(|c| json::object!{
            path: c.path.to_string_lossy().into_owned(),
            startFrame: c.start as isize - 123,
            endFrame: c.end as isize - 123,
        }).collect::<Vec<_>>();

    let out_json = json::object!{
        mode: "queue",
        replay: "",
        queue: queue_json,
    };

    std::fs::write(out_json_path, json::stringify_pretty(out_json, 2))
}

#[derive(Debug)]
pub enum ParsePlaylistError {
    JsonParseError(json::Error),
    NotAPlaylistJsonFile,
}

impl std::fmt::Display for ParsePlaylistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParsePlaylistError::JsonParseError(e) => write!(f, "Invalid json: {}", e),
            ParsePlaylistError::NotAPlaylistJsonFile => write!(f, "File is not a playlist."),
        }
    }
}

pub fn parse_playlist_json(file: &str) -> Result<Vec<Combo>, ParsePlaylistError> {
    let mut parsed = json::parse(file).map_err(|e| ParsePlaylistError::JsonParseError(e))?;

    if parsed["mode"] != "queue" { return Err(ParsePlaylistError::NotAPlaylistJsonFile); }
    if !parsed["queue"].is_array() { return Err(ParsePlaylistError::NotAPlaylistJsonFile); }

    let games = parsed["queue"]
        .members_mut()
        .filter_map(|v| {
            let path = v["path"].take_string()?.into();
            let start = (v["startFrame"].as_i64()? + 123) as usize;
            let end = (v["endFrame"].as_i64()? + 123) as usize;

            Some(Combo { path, start, end })
        }).collect::<Vec<_>>();

    Ok(games)
}
