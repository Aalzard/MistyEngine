//! A small Nelder–Mead simplex minimiser (no derivatives), used to fit the LTC matrices.

/// Minimise `f` from `start`, with initial simplex steps `step`; at most `iterations` steps.
/// Returns the best point found and its value.
pub fn minimize<const N: usize>(
    f: impl Fn(&[f64; N]) -> f64,
    start: [f64; N],
    step: [f64; N],
    iterations: usize,
) -> ([f64; N], f64) {
    let mut simplex: Vec<([f64; N], f64)> = (0..=N)
        .map(|i| {
            let mut p = start;
            if i > 0 {
                p[i - 1] += step[i - 1];
            }
            (p, f(&p))
        })
        .collect();
    let lerp = |a: &[f64; N], b: &[f64; N], t: f64| {
        let mut p = *a;
        for k in 0..N {
            p[k] = a[k] + (b[k] - a[k]) * t;
        }
        p
    };
    for _ in 0..iterations {
        simplex.sort_by(|a, b| a.1.total_cmp(&b.1));
        let (best, worst) = (simplex[0].1, simplex[N].1);
        if (worst - best).abs() <= 1e-12 * best.abs().max(1e-30) {
            break;
        }
        // Centroid of every point but the worst.
        let mut c = [0.0; N];
        for (p, _) in &simplex[..N] {
            for k in 0..N {
                c[k] += p[k] / N as f64;
            }
        }
        let w = simplex[N].0;
        let reflected = lerp(&c, &w, -1.0);
        let fr = f(&reflected);
        if fr < best {
            let expanded = lerp(&c, &w, -2.0);
            let fe = f(&expanded);
            simplex[N] = if fe < fr {
                (expanded, fe)
            } else {
                (reflected, fr)
            };
        } else if fr < simplex[N - 1].1 {
            simplex[N] = (reflected, fr);
        } else {
            // Contract towards the better of the worst point and its reflection.
            let (towards, ft) = if fr < worst {
                (reflected, fr)
            } else {
                (w, worst)
            };
            let contracted = lerp(&c, &towards, 0.5);
            let fc = f(&contracted);
            if fc < ft {
                simplex[N] = (contracted, fc);
            } else {
                // Shrink everything towards the best point.
                let b = simplex[0].0;
                for s in simplex.iter_mut().skip(1) {
                    s.0 = lerp(&b, &s.0, 0.5);
                    s.1 = f(&s.0);
                }
            }
        }
    }
    simplex.sort_by(|a, b| a.1.total_cmp(&b.1));
    simplex[0]
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_the_minimum_of_a_bowl() {
        let f = |p: &[f64; 2]| (p[0] - 1.5).powi(2) + 3.0 * (p[1] + 0.5).powi(2);
        let (p, v) = super::minimize(f, [0.0, 0.0], [0.3, 0.3], 500);
        assert!(
            (p[0] - 1.5).abs() < 1e-4 && (p[1] + 0.5).abs() < 1e-4,
            "{p:?}"
        );
        assert!(v < 1e-8);
    }
}
