#[cfg(feature = "tile-rendering")]
use rand::{Rng, SeedableRng};
#[cfg(feature = "tile-rendering")]
use rand_chacha::ChaCha20Rng;

#[cfg(feature = "tile-rendering")]
#[derive(Debug, Clone, Copy)]
pub struct SigilConfig {
    pub spacing: f32,
    pub stroke_weight: f32,
    pub grid_rows: usize,
    pub grid_cols: usize,
}

#[cfg(feature = "tile-rendering")]
pub fn generate_path(seed: [u8; 32], config: SigilConfig) -> Vec<(f32, f32)> {
    let mut rng = ChaCha20Rng::from_seed(seed);
    let mut points = Vec::new();

    let cols = config.grid_cols;
    let rows = config.grid_rows;

    // Start at a random node
    let start_x = rng.gen_range(0..cols);
    let start_y = rng.gen_range(0..rows);
    let mut curr = (start_x, start_y);

    points.push(grid_to_world(curr, config));

    // Path length between 5 and max nodes
    let len = rng.gen_range(5..=(cols * rows));

    for _ in 0..len {
        // Defined moves (adjacent and diagonal)
        let moves = vec![
            (0, 1),
            (0, -1),
            (1, 0),
            (-1, 0),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ];

        let mut attempts = 0;
        let mut found = false;

        while attempts < 8 {
            let (dx, dy) = moves[rng.gen_range(0..moves.len())];
            let next_x = curr.0 as i32 + dx;
            let next_y = curr.1 as i32 + dy;

            if next_x >= 0 && next_x < cols as i32 && next_y >= 0 && next_y < rows as i32 {
                curr = (next_x as usize, next_y as usize);
                points.push(grid_to_world(curr, config));
                found = true;
                break;
            }
            attempts += 1;
        }

        if !found {
            break;
        }
    }

    points
}

#[cfg(feature = "tile-rendering")]
fn grid_to_world(grid_pos: (usize, usize), config: SigilConfig) -> (f32, f32) {
    // Centering the grid
    let output_x = (grid_pos.0 as f32 - (config.grid_cols as f32 - 1.0) / 2.0) * config.spacing;
    let output_y = (grid_pos.1 as f32 - (config.grid_rows as f32 - 1.0) / 2.0) * config.spacing;
    (output_x, output_y)
}
