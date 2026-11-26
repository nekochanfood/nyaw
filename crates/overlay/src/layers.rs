//! パネルの描画順（レイヤー）。ランタイムは奥行きではなく描画順で重ねるため、順位はここだけで決める。
//! 覚えるのはパネル同士の関係（[`Parent`]・ナビバーの持ち主）だけで、番号は毎 tick 全部振り直す。

use super::*;

/// 相手の面に必ず重ねるパネルの層（下から順）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Layer {
    NavBar,
    Hud,
    Dialog,
    Popover,
}

/// まとまりの奥行きを据え置く幅（m）。距離の差がこの 2 倍を超えて動くまで前後を入れ替えない（ちらつき防止）。
// ponytail: 固定値。実機で入れ替わりがちらつく・遅れるなら調整する。
const DEPTH_SLACK: f32 = 0.01;

/// `p` を必ず重ねる相手と層。頭からの距離で並べる独立したパネルは `None`。
fn lies_on(p: &Panel) -> Option<(PanelId, Layer)> {
    if let Some(owner) = p.nav_bar_of {
        return Some((owner, Layer::NavBar));
    }
    let q = p.parent?;
    let layer = if q.slot.is_some() {
        return None;
    } else if q.hud {
        Layer::Hud
    } else if !q.docked {
        Layer::Dialog
    } else if q.dismiss {
        Layer::Popover
    } else {
        return None;
    };
    Some((q.panel, layer))
}

/// 描くパネルを下から上の順に並べる。相手に重ねるパネル（[`lies_on`]）は相手と一まとまりにして、層・開いた順に積む
/// （子の子は子のすぐ上）。まとまりどうしは、根元の中心が `eye` から遠い順。同じ親にはめた画面と積む子（[`Parent::slot`]）は
/// 移動中に重なるので一つのまとまりにし、退くもの・画面・積む子の順に重ねる。
fn stacking(panels: &mut [Panel], present: &[PanelId], eye: Vec3) -> Vec<PanelId> {
    let mut roots = Vec::new();
    let mut above: HashMap<PanelId, Vec<(Layer, PanelId)>> = HashMap::new();
    for &id in present.iter().filter(|&&id| panels[id].pointer.is_none()) {
        match lies_on(&panels[id]).filter(|(on, _)| present.contains(on)) {
            Some((on, layer)) => above.entry(on).or_default().push((layer, id)),
            None => roots.push(id),
        }
    }
    for v in above.values_mut() {
        v.sort_unstable();
    }
    // まとまりの鍵は、はめた画面なら親、それ以外は自分。
    let mut groups: HashMap<(bool, PanelId), Vec<PanelId>> = HashMap::new();
    for id in roots {
        let key = match panels[id].parent {
            Some(q) if q.slot.is_some() => (true, q.panel),
            _ => (false, id),
        };
        groups.entry(key).or_default().push(id);
    }
    let mut groups: Vec<(f32, PanelId, Vec<PanelId>)> = groups
        .into_values()
        .map(|mut members| {
            members.sort_by_key(|&id| {
                let p = &panels[id];
                (
                    p.parent.is_some_and(|q| q.stack),
                    !(p.closed || p.hidden),
                    id,
                )
            });
            // 奥行きは出ている画面で測る（退くものは奥へ下がっていく）。
            let lead = members
                .iter()
                .copied()
                .find(|&id| !panels[id].closed && !panels[id].hidden)
                .unwrap_or(members[0]);
            let p = &mut panels[lead];
            let far = p.pose.position.distance(eye);
            let depth = p
                .depth
                .map_or(far, |d| d.clamp(far - DEPTH_SLACK, far + DEPTH_SLACK));
            p.depth = Some(depth);
            (depth, lead, members)
        })
        .collect();
    groups.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut order = Vec::with_capacity(present.len());
    for (.., members) in groups {
        for id in members {
            let mut next = vec![id];
            while let Some(id) = next.pop() {
                order.push(id);
                if let Some(v) = above.remove(&id) {
                    next.extend(v.into_iter().rev().map(|(_, c)| c));
                }
            }
        }
    }
    order
}

impl Overlay {
    /// 全パネルの順位を決め、変わったものだけランタイムへ送る。番号は 2 つおきで、すぐ上の空きは代わりの板が使う。
    pub(crate) fn place_layers(&mut self, vr: &mut dyn VrBackend) {
        let order = stacking(&mut self.panels, &self.present, self.hmd.position);
        for (sort, id) in (WORLD_SORT..).step_by(2).zip(order) {
            let surface = &mut self.panels[id].surface;
            if surface.sort() != sort {
                surface.set_sort(vr, sort);
            }
        }
    }
}

#[cfg(test)]
mod tests;
