use std::path::{PathBuf, Path};

const USAGE: &'static str = "Usage: combo_finder <slp or folder path> [out path]";

fn main() {
    let mut args = std::env::args();
    args.next();

    let input_path: PathBuf = match args.next() {
        Some(f) => f.into(),
        None => {
            eprintln!("{}", USAGE);
            std::process::exit(1);
        }
    };

    if !input_path.exists() {
        eprintln!("Error: input path does not exist");
        std::process::exit(1);
    }

    let out_json_path = match args.next() {
        Some(p) => p,
        None => "combos.json".to_string(),
    };

    let config = slp_combo_finder::Config {
        lead_in: 30,
        lead_out: 0,

        player_character: Some(slp_parser::Character::Peach),
        player_code: None,
        player_name: None,
        opponent_character: None,
        opponent_code: None,
        opponent_name: None,
    };

    let combos = slp_combo_finder::target_path(&config, Path::new(&input_path), None).unwrap(); 
    slp_combo_finder::write_playlist(combos.as_slice(), Path::new(&out_json_path)).unwrap()
}