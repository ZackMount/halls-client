//! Controlled memory-scanner regression probe; never part of the player binary.
use halls::{game::Game, player::Player, proof::Recording, world::Map};
use std::io::{BufRead, Write};
fn main() {
    let map = Map::procedural(104729).unwrap();
    let mut game = Game::new(&map);
    let player = Player::new(&map);
    let mut record = Recording::new(&map, &game, &player);
    let start = std::time::Instant::now();
    for _ in 0..20000 {
        game.progress.seconds += 0.025;
        record.tick(&map, &game, &player, 0.025, [0.; 4], 0);
    }
    println!("encrypted {}", start.elapsed().as_micros());
    std::io::stdout().flush().unwrap();
    for line in std::io::stdin().lock().lines() {
        match line.unwrap().as_str() {
            "raw" => {
                std::hint::black_box(record.raw());
                println!("plaintext-control");
            }
            "tick" => {
                record.tick(&map, &game, &player, 0.025, [0.; 4], 0);
                println!("encrypted-again");
            }
            _ => break,
        }
        std::io::stdout().flush().unwrap();
        std::hint::black_box(&record);
    }
}
