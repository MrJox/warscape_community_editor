//! MSVC 2010's `std::sort` (introsort; BOB_Vegetation FUN_10007150 and the functions it calls), element for element: it is not stable, and
//! the order it leaves equal elements in is the order BOB writes the grass list's patches in (which decides their yaws in the game).

/// `std::sort(v.begin(), v.end(), less)`.
pub fn sort<T: Clone>(v: &mut [T], less: &impl Fn(&T, &T) -> bool) {
    let n = v.len();
    sort_range(v, 0, n, n as i64, less);
}

const ISORT_MAX: usize = 32;

fn sort_range<T: Clone>(v: &mut [T], mut first: usize, mut last: usize, mut ideal: i64, less: &impl Fn(&T, &T) -> bool) {
    loop {
        let count = last - first;
        if count <= ISORT_MAX || ideal <= 0 {
            break;
        }

        let (mid_first, mid_last) = unguarded_partition(v, first, last, less);
        ideal = ideal / 2 + (ideal / 2) / 2;
        if mid_first - first < last - mid_last {
            sort_range(v, first, mid_first, ideal, less);
            first = mid_last;
        } else {
            sort_range(v, mid_last, last, ideal, less);
            last = mid_first;
        }
    }

    let count = last - first;
    if count > ISORT_MAX {
        make_heap(v, first, last, less);
        sort_heap(v, first, last, less);
    } else if count > 1 {
        insertion_sort(v, first, last, less);
    }
}

fn med3<T>(v: &mut [T], a: usize, b: usize, c: usize, less: &impl Fn(&T, &T) -> bool) {
    if less(&v[b], &v[a]) {
        v.swap(b, a);
    }

    if less(&v[c], &v[b]) {
        v.swap(c, b);
    }

    if less(&v[b], &v[a]) {
        v.swap(b, a);
    }
}

/// `_Median`: `last` is the last element.
fn median<T>(v: &mut [T], first: usize, mid: usize, last: usize, less: &impl Fn(&T, &T) -> bool) {
    if last - first > 40 {
        let step = (last - first + 1) / 8;
        med3(v, first, first + step, first + 2 * step, less);
        med3(v, mid - step, mid, mid + step, less);
        med3(v, last - 2 * step, last - step, last, less);
        med3(v, first + step, mid, last - step, less);
    } else {
        med3(v, first, mid, last, less);
    }
}

fn unguarded_partition<T>(v: &mut [T], first: usize, last: usize, less: &impl Fn(&T, &T) -> bool) -> (usize, usize) {
    let mid = first + (last - first) / 2;
    median(v, first, mid, last - 1, less);
    let mut pfirst = mid;
    let mut plast = pfirst + 1;
    while first < pfirst && !less(&v[pfirst - 1], &v[pfirst]) && !less(&v[pfirst], &v[pfirst - 1]) {
        pfirst -= 1;
    }

    while plast < last && !less(&v[plast], &v[pfirst]) && !less(&v[pfirst], &v[plast]) {
        plast += 1;
    }

    let mut gfirst = plast;
    let mut glast = pfirst;
    loop {
        while gfirst < last {
            if less(&v[pfirst], &v[gfirst]) {
            } else if less(&v[gfirst], &v[pfirst]) {
                break;
            } else {
                v.swap(plast, gfirst);
                plast += 1;
            }

            gfirst += 1;
        }

        while first < glast {
            if less(&v[glast - 1], &v[pfirst]) {
            } else if less(&v[pfirst], &v[glast - 1]) {
                break;
            } else {
                pfirst -= 1;
                v.swap(pfirst, glast - 1);
            }

            glast -= 1;
        }

        if glast == first && gfirst == last {
            return (pfirst, plast);
        }

        if glast == first {
            // No room at the bottom: the pivot moves up.
            if plast != gfirst {
                v.swap(pfirst, plast);
            }

            plast += 1;
            v.swap(pfirst, gfirst);
            pfirst += 1;
            gfirst += 1;
        } else if gfirst == last {
            // No room at the top: the pivot moves down.
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

fn insertion_sort<T: Clone>(v: &mut [T], first: usize, last: usize, less: &impl Fn(&T, &T) -> bool) {
    let mut next = first + 1;
    while next < last {
        let val = v[next].clone();
        if less(&val, &v[first]) {
            v[first..=next].rotate_right(1);
        } else {
            let mut hole = next;
            while less(&val, &v[hole - 1]) {
                v[hole] = v[hole - 1].clone();
                hole -= 1;
            }

            v[hole] = val;
        }

        next += 1;
    }
}

fn push_heap<T: Clone>(v: &mut [T], first: usize, mut hole: usize, top: usize, val: T, less: &impl Fn(&T, &T) -> bool) {
    while top < hole {
        let idx = (hole - 1) / 2;
        if !less(&v[first + idx], &val) {
            break;
        }

        v[first + hole] = v[first + idx].clone();
        hole = idx;
    }

    v[first + hole] = val;
}

fn adjust_heap<T: Clone>(v: &mut [T], first: usize, mut hole: usize, bottom: usize, val: T, less: &impl Fn(&T, &T) -> bool) {
    let top = hole;
    let mut idx = 2 * hole + 2;
    while idx < bottom {
        if less(&v[first + idx], &v[first + idx - 1]) {
            idx -= 1;
        }

        v[first + hole] = v[first + idx].clone();
        hole = idx;
        idx = 2 * idx + 2;
    }

    if idx == bottom {
        v[first + hole] = v[first + bottom - 1].clone();
        hole = bottom - 1;
    }

    push_heap(v, first, hole, top, val, less);
}

fn make_heap<T: Clone>(v: &mut [T], first: usize, last: usize, less: &impl Fn(&T, &T) -> bool) {
    let bottom = last - first;
    let mut hole = bottom / 2;
    while hole > 0 {
        hole -= 1;
        let val = v[first + hole].clone();
        adjust_heap(v, first, hole, bottom, val, less);
    }
}

fn sort_heap<T: Clone>(v: &mut [T], first: usize, mut last: usize, less: &impl Fn(&T, &T) -> bool) {
    while last - first > 1 {
        let val = v[last - 1].clone();
        v[last - 1] = v[first].clone();
        adjust_heap(v, first, 0, last - 1 - first, val, less);
        last -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_by_key() {
        let mut s = 12345u32;
        let mut v: Vec<(u32, u32)> = (0..5000)
            .map(|i| {
                s = s.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
                ((s >> 16) % 13, i)
            })
            .collect();
        sort(&mut v, &|a: &(u32, u32), b: &(u32, u32)| a.0 < b.0);
        assert!(v.windows(2).all(|w| w[0].0 <= w[1].0));
        let mut ids: Vec<u32> = v.iter().map(|e| e.1).collect();
        ids.sort_unstable();
        assert_eq!(ids, (0..5000).collect::<Vec<_>>());
    }
}
