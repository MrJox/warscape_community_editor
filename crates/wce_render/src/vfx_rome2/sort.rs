//! The particle sort of Rome 2's VFX views: the game's `std::sort` (MSVC 2012) of (camera distance squared, index) pairs, with the
//! predicate "a's key is greater" (back to front). Equal keys are common (particles spawned on one point without speed), and their
//! order is the algorithm's: so it is ported as it is (fn 0x11578dd0 and its helpers), not replaced by another sort.

/// One sort item: the key and the index of the record it stands for.
pub type Item = (f32, u32);

fn pred(a: &Item, b: &Item) -> bool {
    a.0 > b.0
}

/// Sorts `v` as the game does (fn 0x11578dd0, called with the count as the depth budget).
pub fn sort(v: &mut [Item]) {
    let n = v.len() as isize;
    sort_range(v, 0, v.len(), n);
}

/// `_Sort`: introsort; at most 32 elements are left to the insertion sort; out of budget, a heap sort.
fn sort_range(v: &mut [Item], mut first: usize, mut last: usize, mut ideal: isize) {
    loop {
        let count = last - first;
        if count <= 32 {
            break;
        }

        if ideal <= 0 {
            make_heap(v, first, last);
            sort_heap(v, first, last);
            return;
        }

        let (mid_first, mid_last) = unguarded_partition(v, first, last);
        ideal = (ideal >> 1) + (ideal >> 2);
        if mid_first - first < last - mid_last {
            sort_range(v, first, mid_first, ideal);
            first = mid_last;
        } else {
            sort_range(v, mid_last, last, ideal);
            last = mid_first;
        }
    }

    if last - first > 1 {
        insertion_sort(v, first, last);
    }
}

/// `_Insertion_sort`: an element before the first goes to the front; the others shift down to their place.
fn insertion_sort(v: &mut [Item], first: usize, last: usize) {
    for next in first + 1..last {
        let val = v[next];
        if pred(&val, &v[first]) {
            v.copy_within(first..next, first + 1);
            v[first] = val;
        } else {
            let mut hole = next;
            while pred(&val, &v[hole - 1]) {
                v[hole] = v[hole - 1];
                hole -= 1;
            }

            v[hole] = val;
        }
    }
}

/// `_Med3` (fn 0x115726b0's three-element form).
fn med3(v: &mut [Item], a: usize, b: usize, c: usize) {
    if pred(&v[b], &v[a]) {
        v.swap(b, a);
    }

    if pred(&v[c], &v[b]) {
        v.swap(c, b);
        if pred(&v[b], &v[a]) {
            v.swap(b, a);
        }
    }
}

/// `_Median` (fn 0x115726b0): of three, or of nine (Tukey's ninther) past 40 elements.
fn median(v: &mut [Item], first: usize, mid: usize, last: usize) {
    if last - first > 40 {
        let step = (last - first + 1) / 8;
        med3(v, first, first + step, first + 2 * step);
        med3(v, mid - step, mid, mid + step);
        med3(v, last - 2 * step, last - step, last);
        med3(v, first + step, mid, last - step);
    } else {
        med3(v, first, mid, last);
    }
}

/// `_Unguarded_partition` (fn 0x11575da0): the pivot's equal range [pfirst, plast) in the middle, the greater keys before it.
fn unguarded_partition(v: &mut [Item], first: usize, last: usize) -> (usize, usize) {
    let mid = first + (last - first) / 2;
    median(v, first, mid, last - 1);
    let mut pfirst = mid;
    let mut plast = pfirst + 1;
    while first < pfirst && !pred(&v[pfirst - 1], &v[pfirst]) && !pred(&v[pfirst], &v[pfirst - 1]) {
        pfirst -= 1;
    }

    while plast < last && !pred(&v[plast], &v[pfirst]) && !pred(&v[pfirst], &v[plast]) {
        plast += 1;
    }

    let mut gfirst = plast;
    let mut glast = pfirst;
    loop {
        while gfirst < last {
            if pred(&v[pfirst], &v[gfirst]) {
            } else if pred(&v[gfirst], &v[pfirst]) {
                break;
            } else {
                if plast != gfirst {
                    v.swap(plast, gfirst);
                }

                plast += 1;
            }

            gfirst += 1;
        }

        while first < glast {
            if pred(&v[glast - 1], &v[pfirst]) {
            } else if pred(&v[pfirst], &v[glast - 1]) {
                break;
            } else {
                pfirst -= 1;
                if pfirst != glast - 1 {
                    v.swap(pfirst, glast - 1);
                }
            }

            glast -= 1;
        }

        if glast == first && gfirst == last {
            return (pfirst, plast);
        }

        if glast == first {
            // No room at the bottom: the pivot range moves up.
            if plast != gfirst {
                v.swap(pfirst, plast);
            }

            plast += 1;
            v.swap(pfirst, gfirst);
            pfirst += 1;
            gfirst += 1;
        } else if gfirst == last {
            // No room at the top: the pivot range moves down.
            glast -= 1;
            pfirst -= 1;
            if glast != pfirst {
                v.swap(glast, pfirst);
            }

            plast -= 1;
            v.swap(pfirst, plast);
        } else {
            glast -= 1;
            v.swap(gfirst, glast);
            gfirst += 1;
        }
    }
}

/// `_Adjust_heap` (fn 0x115774b0): `val` sifted down from `hole` in the heap of `bottom` elements at `base`, then pushed back up.
fn adjust_heap(v: &mut [Item], base: usize, mut hole: usize, bottom: usize, val: Item) {
    let top = hole;
    let half = (bottom as isize - 1) >> 1;
    while (hole as isize) < half {
        let mut idx = 2 * hole + 2;
        if pred(&v[base + idx], &v[base + idx - 1]) {
            idx -= 1;
        }

        v[base + hole] = v[base + idx];
        hole = idx;
    }

    if hole as isize == half && bottom.is_multiple_of(2) {
        v[base + hole] = v[base + bottom - 1];
        hole = bottom - 1;
    }

    while top < hole {
        let parent = (hole - 1) >> 1;
        if !pred(&v[base + parent], &val) {
            break;
        }

        v[base + hole] = v[base + parent];
        hole = parent;
    }

    v[base + hole] = val;
}

fn make_heap(v: &mut [Item], first: usize, last: usize) {
    let count = last - first;
    let mut hole = count / 2;
    while hole > 0 {
        hole -= 1;
        let val = v[first + hole];
        adjust_heap(v, first, hole, count, val);
    }
}

fn sort_heap(v: &mut [Item], first: usize, last: usize) {
    let mut count = last - first;
    while count > 1 {
        let val = v[first + count - 1];
        v[first + count - 1] = v[first];
        adjust_heap(v, first, 0, count - 1, val);
        count -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lcg(state: &mut u32) -> u32 {
        *state = state.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        *state >> 16
    }

    #[test]
    fn sorts_back_to_front() {
        for (n, levels) in [(0usize, 1u32), (1, 1), (5, 3), (33, 4), (41, 7), (200, 9), (1000, 50), (5000, 3)] {
            let mut seed = n as u32 + 1;
            let mut v: Vec<Item> = (0..n as u32).map(|i| ((lcg(&mut seed) % levels) as f32, i)).collect();
            sort(&mut v);
            assert!(v.windows(2).all(|w| w[0].0 >= w[1].0), "{n} items not in order");
            let mut idx: Vec<u32> = v.iter().map(|x| x.1).collect();
            idx.sort_unstable();
            assert!(idx.iter().copied().eq(0..n as u32), "{n} items lost or doubled");
        }
    }

    #[test]
    fn heap_sort_when_out_of_budget() {
        // A budget of 0 goes straight to the heap sort.
        let mut seed = 9;
        let mut v: Vec<Item> = (0..100u32).map(|i| ((lcg(&mut seed) % 13) as f32, i)).collect();
        let n = v.len();
        sort_range(&mut v, 0, n, 0);
        assert!(v.windows(2).all(|w| w[0].0 >= w[1].0));
    }

    #[test]
    fn small_ties_keep_insertion_order() {
        // The insertion sort moves an element only past smaller keys: equal keys keep their order.
        let mut v: Vec<Item> = [(1.0, 0), (2.0, 1), (1.0, 2), (2.0, 3), (1.0, 4)].to_vec();
        sort(&mut v);
        assert_eq!(v, [(2.0, 1), (2.0, 3), (1.0, 0), (1.0, 2), (1.0, 4)]);
    }
}
