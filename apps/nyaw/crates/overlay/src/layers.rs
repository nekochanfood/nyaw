//! パネルの描画順と重なりの奥行き（レイヤー）。ランタイムは奥行きではなく描画順で重ねるため、順位はここだけで決める。
//! 覚えるのはパネル同士の関係（[`Parent`]・ナビバーの持ち主）だけで、番号は毎 tick 全部振り直す。
//!
//! 相手の面からどれだけ手前に置くか（相対姿勢の Z）は呼び出し元の要望で、確定値はここで決める。重なる下のパネルより
//! 手前になければ、[`LIFT_STEP`] 手前までずらす。要望（`offset`）とずらした量（`lift`）は分けて持ち、混ぜない。

use super::*;

/// 相手の面に必ず重ねるパネルの層（下から順）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Layer {
    NavBar,
    Hud,
    Dialog,
    Popover,
    /// 親の面の一部に重ねる子（[`Parent::over`]）。
    Over,
}

/// まとまりの奥行きを据え置く幅（m）。距離の差がこの 2 倍を超えて動くまで前後を入れ替えない（ちらつき防止）。
// ponytail: 固定値。実機で入れ替わりがちらつく・遅れるなら調整する。
const DEPTH_SLACK: f32 = 0.01;
/// 手前にずらす量の上限（m）。これより奥を要望したパネルは、離して置いたものとして動かさない。
// ponytail: 固定値。上限があるので、ずらしたまま掴んでも要望との差は小さい。
const LIFT_REACH: f32 = 0.02;

/// `p` の面を基準にした `q` の中心。面が平行でなければ（起こして立てたもの）重なりを比べないので `None`。
fn on_face(p: &Panel, q: &Panel) -> Option<Vec3> {
    (p.pose.orientation.dot(q.pose.orientation).abs() > 0.9999)
        .then(|| p.pose.orientation.inverse() * (q.pose.position - p.pose.position))
}

/// `order`（[`stacking`] の順）のパネルを、重ねる相手とそれまでに積んだものより手前にする。掴んでいるパネル
/// （`held`）は手に従うので、ずらした量を変えない。
fn lift(panels: &mut [Panel], order: &[PanelId], held: &[PanelId]) {
    for (i, &id) in order.iter().enumerate() {
        if held.contains(&id) {
            continue;
        }
        panels[id].lift = 0.0;
        let Some((on, _)) = lies_on(&panels[id]) else {
            continue;
        };
        // 比べるのは、相手（か相手にはめた画面）から自分の手前までに積んだもの。
        let from = order[..i].iter().position(|&b| {
            b == on
                || panels[b]
                    .parent
                    .is_some_and(|q| q.panel == on && q.slot.is_some())
        });
        let Some(from) = from else {
            continue;
        };
        let (face, p) = (&panels[on], &panels[id]);
        let Some(at) = on_face(face, p) else {
            continue;
        };
        let floor = order[from..i]
            .iter()
            .filter_map(|&b| {
                // 隠して消えきったものは、重なる相手に数えない。
                let q = Some(&panels[b]).filter(|q| b == on || !q.hidden || q.opacity > 0.0)?;
                let below = on_face(face, q)?;
                let gap = (below - at).truncate().abs() * 2.0;
                gap.cmplt(p.size + q.size)
                    .all()
                    .then_some(below.z + LIFT_STEP)
            })
            .reduce(f32::max);
        let Some(by) = floor
            .map(|floor| floor - at.z)
            .filter(|&by| by > 0.0 && by <= LIFT_REACH)
        else {
            continue;
        };
        let normal = face.pose.orientation * Vec3::Z;
        let p = &mut panels[id];
        p.pose.position += normal * by;
        p.lift = by;
    }
}

/// `p` を必ず重ねる相手と層。頭からの距離で並べる独立したパネルは `None`。
fn lies_on(p: &Panel) -> Option<(PanelId, Layer)> {
    if let Some(owner) = p.nav_bar_of {
        return Some((owner, Layer::NavBar));
    }
    let q = p.parent?;
    let layer = if q.slot.is_some() {
        return None;
    } else if q.over.is_some() {
        Layer::Over
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
/// 移動中に重なるので一つのまとまりにし、退くもの・画面・積む子の順に重ねる。画面をはめた親が開くダイアログ・
/// ポップオーバー・面に重ねる子は、親から立ち上がって画面にも重なるので、親と画面の両方を積んだ後に積む
/// （親と画面の前後は距離のまま）。
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
    // はめた画面のある親の、画面にも重なる子は取り分けておく。
    let mut raised = Vec::new();
    for (&on, v) in &mut above {
        let docked = |&id: &PanelId| {
            panels[id]
                .parent
                .is_some_and(|q| q.panel == on && q.slot.is_some())
        };
        if roots.iter().any(docked) {
            let from = v.partition_point(|&(layer, _)| layer < Layer::Dialog);
            raised.push((on, v.split_off(from)));
        }
    }
    raised.sort_unstable_by_key(|&(on, _)| on);
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
    // 画面のまとまりを積み終えた親。
    let mut screens = Vec::new();
    for (.., members) in groups {
        if let Some(q) = panels[members[0]].parent.filter(|q| q.slot.is_some()) {
            screens.push(q.panel);
        }
        for id in members {
            pile(id, &mut above, &mut order);
        }
        while let Some(i) = raised
            .iter()
            .position(|(on, _)| screens.contains(on) && order.contains(on))
        {
            for (_, id) in raised.remove(i).1 {
                pile(id, &mut above, &mut order);
            }
        }
    }
    order
}

/// `id` と、その上に重ねるもの（子の子は子のすぐ上）を `order` に積む。
fn pile(
    id: PanelId,
    above: &mut HashMap<PanelId, Vec<(Layer, PanelId)>>,
    order: &mut Vec<PanelId>,
) {
    let mut next = vec![id];
    while let Some(id) = next.pop() {
        order.push(id);
        if let Some(v) = above.remove(&id) {
            next.extend(v.into_iter().rev().map(|(_, c)| c));
        }
    }
}

impl Overlay {
    /// 全パネルの順位と重なりの奥行きを決め、順位が変わったものだけランタイムへ送る。番号は 2 つおきで、すぐ上の空きは
    /// 代わりの板が使う。掴んでいるパネル（`held`）以外の姿勢が出そろってから呼ぶ。
    pub(crate) fn place_layers(&mut self, vr: &mut dyn VrBackend, held: &[PanelId]) {
        let order = stacking(&mut self.panels, &self.present, self.hmd.position);
        lift(&mut self.panels, &order, held);
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
