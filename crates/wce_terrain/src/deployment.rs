//! The deployment files BOB_Terrain's `Tilemap` writes (`deployment/deployment_NNNN.xml`, `BATTLE_DEPLOYMENT_AREA_HASH_TABLE`; docs/re/
//! terry.md, "The scene's structure"): one per battle catchment area with deployment zones, in the `.terry`'s entity order; per alliance
//! (0, 1) a `deployment_area` per zone of that alliance (its `deployment_zone_id`, its category `DZC_*` as `DAC_*`), per region of the
//! zone a `boundary` (`standard additive` or `standard subtractive`) of its polyline's points relative to the BCA's position (metres, y
//! as z), printed `%f`. Tabs, LF line endings.

use crate::terry::{EntityKind, TerryProject};

/// `%f` as BOB's C runtime prints it: six decimals, an exact half rounded away from zero (Rust's `{:.6}` rounds it to even).
pub fn printf_f(v: f64) -> String {
    // The exact decimal expansion (an f32's value has at most 24 binary fractional digits that matter here), then rounded by hand.
    let s = format!("{:.40}", v.abs());
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let mut digits: Vec<u8> = int.bytes().chain(frac.bytes().take(6)).map(|c| c - b'0').collect();
    let rest = &frac.as_bytes()[6.min(frac.len())..];
    if rest.first().is_some_and(|&c| c >= b'5') {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, 1);
                break;
            }

            i -= 1;
            if digits[i] == 9 {
                digits[i] = 0;
            } else {
                digits[i] += 1;
                break;
            }
        }
    }

    let n = digits.len() - 6;
    let text: String = digits[..n].iter().map(|d| char::from(b'0' + d)).collect::<String>()
        + "."
        + &digits[n..].iter().map(|d| char::from(b'0' + d)).collect::<String>();
    if v.is_sign_negative() && v != 0.0 { format!("-{text}") } else { text }
}

fn f(v: f64) -> String {
    printf_f(v)
}

#[cfg(test)]
mod tests {
    use super::printf_f;

    #[test]
    fn rounds_as_msvc() {
        assert_eq!(printf_f(-121.8203125), "-121.820313");
        assert_eq!(printf_f(512.0078125), "512.007813");
        assert_eq!(printf_f(41.65625), "41.656250");
        assert_eq!(printf_f(-958.0), "-958.000000");
        assert_eq!(printf_f(0.0), "0.000000");
        assert_eq!(printf_f(9.9999999), "10.000000");
        assert_eq!(printf_f(-0.0000001), "-0.000000");
    }
}

/// A project's deployment files: (`deployment_NNNN.xml`, bytes), numbered from 0.
pub fn build(p: &TerryProject) -> Vec<(String, Vec<u8>)> {
    let mut out = vec![];
    for e in &p.entities {
        if !matches!(p.kind(e), EntityKind::Bca(_)) {
            continue;
        }

        let zones: Vec<_> =
            e.children.iter().filter_map(|&c| p.entity(c)).filter(|z| matches!(p.kind(z), EntityKind::DeploymentZone)).collect();
        if zones.is_empty() {
            continue;
        }

        let origin = p.world_position(e.id);
        let mut x = String::from("<BATTLE_DEPLOYMENT_AREA_HASH_TABLE>\n\t<BATTLE_DEPLOYMENT_AREAS>\n");
        for alliance in 0..2 {
            x += &format!("\t\t<ALLIANCE id='{alliance}'>\n");
            for z in &zones {
                let Some(c) = p.component(z, "ECDeploymentZone") else { continue };
                let attr = |a: &str| p.doc.get(c, a).unwrap_or_default();
                if attr("alliance_id").parse::<i32>().ok() != Some(alliance) {
                    continue;
                }

                let category = attr("deployment_category");
                let category = category.strip_prefix("DZC_").unwrap_or(&category);
                x += &format!("\t\t\t<deployment_area id='{}' category='DAC_{category}'>\n", attr("deployment_zone_id"));
                for r in z.children.iter().filter_map(|&c| p.entity(c)) {
                    let (Some(rc), Some((pts, _))) = (p.component(r, "ECDeploymentZoneRegion"), p.polyline(r)) else { continue };
                    let kind = if p.doc.get(rc, "region_type").as_deref() == Some("DZRT_SUBTRACTIVE") { "subtractive" } else { "additive" };
                    let w = p.world_position(r.id);
                    x += &format!("\t\t\t\t<boundary type='standard {kind}'>\n");
                    for q in &pts {
                        // In f32 as BOB: the point on the map, then relative to the BCA.
                        let px = (w[0] as f32 + q[0] as f32) - origin[0] as f32;
                        let pz = (w[2] as f32 + q[1] as f32) - origin[2] as f32;
                        x += &format!("\t\t\t\t\t<position x='{}' y='{}'/>\n", f(f64::from(px)), f(f64::from(pz)));
                    }

                    x += "\t\t\t\t</boundary>\n";
                }

                x += "\t\t\t</deployment_area>\n";
            }

            x += "\t\t</ALLIANCE>\n";
        }

        x += "\t</BATTLE_DEPLOYMENT_AREAS>\n</BATTLE_DEPLOYMENT_AREA_HASH_TABLE>\n";
        out.push((format!("deployment_{:04}.xml", out.len()), x.into_bytes()));
    }

    out
}
