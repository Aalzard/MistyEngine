//! Monotone partitioning of a tile's cells into regions: each row is cut into runs of connected
//! cells with one area, and a run joins the region of the row before when that region continues
//! into this run only. Regions never have holes and never mix areas. Cells in the tile's border
//! get no region (0), so outlines follow the tile edge.

use super::heightfield::{CompactField, NULL_AREA};

/// The region of each cell (0 = none) and the area of each region (index = id).
pub(super) struct Regions {
    pub of_cell: Vec<u32>,
    pub area: Vec<u8>,
}

impl Regions {
    pub fn count(&self) -> usize {
        self.area.len()
    }
}

#[derive(Clone, Copy, Default)]
struct Sweep {
    /// Region of the row before this run connects to (0 = none, `MANY` = several).
    neighbour: u32,
    /// Cells of the run touching `neighbour`.
    touching: u32,
    id: u32,
}

const MANY: u32 = u32::MAX;

pub(super) fn build(f: &CompactField) -> Regions {
    let n = f.cells.len();
    let mut of_cell = vec![0u32; n];
    let mut area = vec![NULL_AREA];
    // Per region id: cells of the current row touching it from any run.
    let mut prev_count: Vec<u32> = vec![0];
    let usable = |i: u32| {
        let c = &f.cells[i as usize];
        c.area != NULL_AREA && f.in_core(c)
    };
    for z in 0..f.depth as i32 {
        // Row-local run ids start at 1 (index 0 unused).
        let mut sweeps: Vec<Sweep> = vec![Sweep::default()];
        prev_count.iter_mut().for_each(|c| *c = 0);
        let mut row_cells: Vec<u32> = Vec::new();
        for x in 0..f.width as i32 {
            for i in f.column(x, z) {
                if !usable(i) {
                    continue;
                }
                let c = f.cells[i as usize];
                // Continue the run of the -x neighbour.
                let mut run = 0;
                if let Some(l) = f.neighbour(i, 0)
                    && usable(l)
                    && f.cells[l as usize].area == c.area
                    && of_cell[l as usize] != 0
                    && f.cells[l as usize].z == c.z
                {
                    run = of_cell[l as usize];
                }
                if run == 0 {
                    run = sweeps.len() as u32;
                    sweeps.push(Sweep::default());
                }
                // Region of the -z neighbour (already final).
                if let Some(b) = f.neighbour(i, 3)
                    && usable(b)
                    && f.cells[b as usize].area == c.area
                {
                    let r = of_cell[b as usize];
                    if r != 0 {
                        let s = &mut sweeps[run as usize];
                        if s.neighbour == 0 || s.neighbour == r {
                            s.neighbour = r;
                            s.touching += 1;
                            prev_count[r as usize] += 1;
                        } else {
                            s.neighbour = MANY;
                        }
                    }
                }
                of_cell[i as usize] = run;
                row_cells.push(i);
            }
        }
        // Runs whose only neighbour region continues into them alone take its id.
        for s in sweeps.iter_mut().skip(1) {
            if s.neighbour != 0
                && s.neighbour != MANY
                && prev_count[s.neighbour as usize] == s.touching
            {
                s.id = s.neighbour;
            } else {
                s.id = area.len() as u32;
                area.push(NULL_AREA);
                prev_count.push(0);
            }
        }
        for i in row_cells {
            let s = sweeps[of_cell[i as usize] as usize];
            of_cell[i as usize] = s.id;
            area[s.id as usize] = f.cells[i as usize].area;
        }
    }
    Regions { of_cell, area }
}
