use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

#[derive(Debug, Clone, Copy)]
pub enum PlanetarySphere {
    Saturn = 3,  // 3x3: Binding, Time, Endings
    Jupiter = 4, // 4x4: Wealth, Law, Expansion (Default)
    Mars = 5,    // 5x5: War, Conflict, Protection
    Sun = 6,     // 6x6: Health, Ego, Success
    Venus = 7,   // 7x7: Love, Harmony
    Mercury = 8, // 8x8: Code, Speed, Comm
    Moon = 9,    // 9x9: Intuition, Home, Dreams
}

#[derive(Debug, Clone, Copy)]
pub struct SigilConfig {
    pub spacing: f32,
    pub stroke_weight: f32,
    pub grid_rows: usize,
    pub grid_cols: usize,
}

impl Default for SigilConfig {
    fn default() -> Self {
        Self {
            spacing: 150.0,
            stroke_weight: 8.0,
            grid_rows: 4,
            grid_cols: 4,
        }
    }
}

pub fn generate_path(seed_bytes: [u8; 32], config: SigilConfig) -> Vec<(f32, f32)> {
    let grid_size = config.grid_rows * config.grid_cols;
    
    // Scale path length with grid size
    let min_len = config.grid_rows + 2;
    let max_len = (config.grid_rows * 2) + 2;
    
    let mut rng = ChaCha8Rng::from_seed(seed_bytes);
    let path_length = rng.gen_range(min_len..max_len);
    
    let mut path_indices = Vec::new();

    // Consume hash bytes to walk the grid
    for i in 0..path_length {
        let byte_idx = i % 32;
        let mut idx = (seed_bytes[byte_idx] as usize) % grid_size;

        // Prevent immediate backtracking or staying still
        if let Some(last) = path_indices.last() {
            if *last == idx {
                idx = (idx + 1) % grid_size;
            }
        }
        path_indices.push(idx);
    }
    
    // Convert indices to Screen Coordinates
    let cols = config.grid_cols as f32;
    let rows = config.grid_rows as f32;
    let spacing = config.spacing;
    
    let offset_x = -((cols - 1.0) * spacing) / 2.0;
    let offset_y = -((rows - 1.0) * spacing) / 2.0; 
    
    path_indices.iter().map(|&idx| {
        let col = (idx % config.grid_cols) as f32;
        let row = (idx / config.grid_cols) as f32; 
        
        let x = offset_x + col * spacing;
        let y = offset_y + row * spacing; 
        (x, y)
    }).collect()
}
