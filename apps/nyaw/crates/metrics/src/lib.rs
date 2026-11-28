//! パネルの寸法。配置を計算するオーバーレイ層と、描画するデザイン層（`design` が再エクスポートする）が共有する。

/// パネルの影のぼかし（ガウスの σ）。オフセットなしで四方へ広がる影により、重なったパネル間の境界を示す。
pub const PANEL_SHADOW_BLUR: f32 = 8.0;
/// パネル背景をウィンドウ端から離す幅。影はパネルのテクスチャ内に描画するため、この範囲に収める。
/// GPUI は影を σ の 3 倍まで描画するため、同値にして影が途切れないようにする。
pub const PANEL_MARGIN: f32 = 3.0 * PANEL_SHADOW_BLUR;
/// パネルの角丸の半径（px）。`design` の `radius::XL` と一致させる（`design` がビルド時に検査する）。
pub const PANEL_RADIUS: f32 = 24.0;
/// 角枠（beam の `bracket`。はまる所の見本などが使う、角丸矩形の隅だけを囲む線）の太さ（px）。
pub const BRACKET_LINE_PX: f32 = 6.0;
/// パネル面から角枠の外側までの幅（px）。面と線の間に、線の太さの 2 倍の隙間を空ける。
pub const BRACKET_PAD_PX: f32 = 3.0 * BRACKET_LINE_PX;
/// 角を囲む線（パネルの角を掴んで大きさを変えるときの目印）の窓の一辺（px）。オーバーレイ層は窓の中心をパネル面の角に置き、
/// 窓 1px がパネル 1px になる大きさにする。Windows の非表示ウィンドウの最小サイズ制約があるため、これより小さくしない。
pub const CORNER_HANDLE_PX: u32 = 240;
