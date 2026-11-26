// Virtual Museum — yaxlit 3D muzey (three.js)
//
// Reja (yuqoridan, shimol = yuqori):
//
//        [zal 3] |  koridor  | [zal 4]
//        [zal 1] |           | [zal 2]
//                +--+     +--+
//                |   LOBBI   |
//                +--[kirish]-+
//
// Bino bazadagi zallar va asarlardan quriladi. Koordinatalar: x — sharq(+)/g'arb(-),
// n — shimolga masofa (three.js da z = -n).
import * as THREE from "three";

const DATA = JSON.parse(document.getElementById("museum-data").textContent);
window.museum = DATA;
const museum = DATA;
const stage = document.getElementById("stage");
const TEX = stage.dataset.textures;
const canvas = document.getElementById("view");

// Grafika sifati: "high" — yo'naltirilgan chiroqlar, silliqlash; "low" — kuchsiz qurilmalar uchun
const QUALITY = (() => { try { return localStorage.getItem("museumQuality") || "high"; } catch { return "high"; } })();
const HIGH = QUALITY !== "low";

// Yozuvlar chiroyli shriftda chizilishi uchun shriftlar yuklanishini qisqa kutamiz
try {
  await Promise.race([
    Promise.all([document.fonts.load('600 64px "Cormorant Garamond"'), document.fonts.load('500 32px "Inter"')]),
    new Promise((r) => setTimeout(r, 2500)),
  ]);
} catch { /* shriftsiz ham ishlaydi */ }

// ---------------------------------------------------------------- o'lchamlar
const H = 5.5;          // shift balandligi
const T = 0.3;          // devor qalinligi
const DOOR = 4;         // eshik kengligi
const DOOR_H = 3.9;     // eshik balandligi
const EYE = 1.65;       // ko'z balandligi
const R = 0.35;         // tashrifchi "radiusi" (to'qnashuv uchun)
const CW = 6;           // koridor kengligi
const D = 14;           // zal chuqurligi
const GAP = 1.2;        // qo'shni zallar orasidagi bo'shliq
const LW = 18, LL = 14; // lobbi eni / uzunligi
const HL = 14;          // zal uzunligi (har bir zalda 8 ta o'rin)
const PMAX_W = 2.4, PMAX_H = 2.0, PY = 2.45;
const PED_H = 0.9;      // haykal postamenti balandligi
const SMAX_W = 1.6, SMAX_H = 2.1; // haykalning eng katta eni / bo'yi (odamga yaqinroq proporsiya)
const RAIL = 0.92;      // devor panelining balandligi

// ---------------------------------------------------------------- renderer
let renderer;
try {
  renderer = new THREE.WebGLRenderer({ canvas, antialias: HIGH, powerPreference: "high-performance" });
} catch (e1) {
  console.warn("High-performance WebGL context failed, retrying default:", e1);
  try {
    renderer = new THREE.WebGLRenderer({ canvas, antialias: false });
  } catch (e2) {
    console.error("WebGL context creation failed completely:", e2);
    const el = document.getElementById("loadstate");
    if (el) {
      el.innerHTML = "<span style='color:#ff7675;font-weight:bold'>3D dvigatel ishga tushmadi: WebGL qo'llab-quvvatlanmadi.</span><br><small style='color:#ddd'>Brauzer sozlamalarida 'Grafik tezlashtirish' (Hardware Acceleration) yoqilganligini tekshiring.</small>";
    }
    throw e2;
  }
}
renderer.setPixelRatio(Math.min(window.devicePixelRatio, HIGH ? 1.75 : 1));
renderer.outputColorSpace = THREE.SRGBColorSpace;
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.toneMappingExposure = 1.1;

const scene = new THREE.Scene();
scene.background = new THREE.Color(0x0b0d14);

const camera = new THREE.PerspectiveCamera(70, 1, 0.05, 400);
const player = new THREE.Object3D();
player.add(camera);
scene.add(player);

// Atrof-muhit yoritilishi (IBL): yorug' shiftli xona "suratidan" — marmar va oltin shu bilan yaltiraydi
scene.environment = (() => {
  const env = new THREE.Scene();
  const room = new THREE.Mesh(new THREE.BoxGeometry(12, 6, 12), new THREE.MeshBasicMaterial({ color: 0x6f665b, side: THREE.BackSide }));
  room.position.y = 3;
  env.add(room);
  const light = (w, h, x, y, z, rx, ry, k) => {
    const m = new THREE.Mesh(new THREE.PlaneGeometry(w, h), new THREE.MeshBasicMaterial({
      color: new THREE.Color(0xfff1dc).multiplyScalar(k), side: THREE.DoubleSide,
    }));
    m.position.set(x, y, z);
    m.rotation.set(rx, ry, 0);
    env.add(m);
  };
  light(6, 6, 0, 5.95, 0, Math.PI / 2, 0, 3.2);
  light(2.5, 3, -5.95, 3, 0, 0, Math.PI / 2, 1.2);
  light(2.5, 3, 5.95, 3, 0, 0, -Math.PI / 2, 1.2);
  light(3, 2, 0, 3, -5.95, 0, 0, 0.9);
  const pm = new THREE.PMREMGenerator(renderer);
  const t = pm.fromScene(env, 0.04).texture;
  pm.dispose();
  return t;
})();
scene.add(new THREE.HemisphereLight(0xfff2de, 0x2b2620, 0.35));

const loader = new THREE.TextureLoader();
const aniso = Math.min(8, renderer.capabilities.getMaxAnisotropy());

// ---------------------------------------------------------------- materiallar
const solids = [];   // to'qnashuv to'rtburchaklari {minX,maxX,minZ,maxZ}
const rooms = [];    // HUD va mini-xarita uchun
const clickable = [];
const litSpots = []; // har bir asar uchun chiroq o'rni va nishoni

const texCache = {};
function tex(name) {
  if (!texCache[name]) {
    const t = loader.load(TEX + name + ".png");
    t.wrapS = t.wrapT = THREE.RepeatWrapping;
    t.colorSpace = THREE.SRGBColorSpace;
    t.anisotropy = aniso;
    texCache[name] = t;
  }
  return texCache[name];
}
function canvasTex(c, repeat = false) {
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = aniso;
  if (repeat) t.wrapS = t.wrapT = THREE.RepeatWrapping;
  return t;
}
function normalCanvasTex(c) {
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.NoColorSpace;
  t.anisotropy = aniso;
  return t;
}
const std = (o) => new THREE.MeshStandardMaterial(o);
const matCache = {};
function cached(key, make) { return matCache[key] || (matCache[key] = make()); }

const M = {
  gold: std({ color: 0xd1a94a, metalness: 1, roughness: 0.3 }),
  brass: std({ color: 0xb8913f, metalness: 1, roughness: 0.42 }),
  wood: std({ map: tex("wood_dark"), roughness: 0.5, color: 0xb59a86 }),
  black: std({ color: 0x141416, roughness: 0.45, metalness: 0.2 }),
  ivory: std({ color: 0xece5d6, roughness: 0.45 }),
  marble: std({ map: tex("floor_marble"), color: 0xf2eee6, roughness: 0.25 }),
  ceiling: new THREE.MeshBasicMaterial({ map: tex("ceiling"), color: 0xb9b2a5 }),
  matboard: std({ color: 0xf4f1ea, roughness: 0.9 }),
  ceramic: std({ color: 0x23252b, roughness: 0.35, metalness: 0.1 }),
  velvetRed: std({ color: 0x6d1420, roughness: 1 }),
  lamp: new THREE.MeshBasicMaterial({ color: 0xfff3dc }),
  leaf: [0x2f6b3f, 0x3a7d48, 0x285c36].map((c) => std({ color: c, roughness: 0.8, flatShading: true })),
};
const floorMat = (name) => cached("floor|" + name, () => std({ map: tex(name || "floor_marble"), roughness: name === "floor_dark" ? 0.22 : 0.18 }));
const wallMat = (name, tint) => cached(`wall|${name}|${tint}`, () => std({ map: tex(name), color: new THREE.Color(tint), roughness: 0.92 }));
const plainMat = (color, rough = 0.85) => cached(`plain|${color}|${rough}`, () => std({ color: new THREE.Color(color), roughness: rough }));

function scaleUV(geo, su, sv) {
  const uv = geo.attributes.uv;
  for (let i = 0; i < uv.count; i++) uv.setXY(i, uv.getX(i) * su, uv.getY(i) * sv);
  uv.needsUpdate = true;
  return geo;
}

function box(cx, cy, n, sx, sy, sz, material, solid = false, uv = null) {
  const geo = new THREE.BoxGeometry(sx, sy, sz);
  if (uv) scaleUV(geo, uv[0], uv[1]);
  const m = new THREE.Mesh(geo, material);
  m.position.set(cx, cy, -n);
  scene.add(m);
  if (solid) addSolid(cx - sx / 2, cx + sx / 2, -n - sz / 2, -n + sz / 2);
  return m;
}
function addSolid(minX, maxX, minZ, maxZ) { solids.push({ minX, maxX, minZ, maxZ }); }

function flat(x1, x2, n1, n2, material, y, up, rep = 3) {
  const w = x2 - x1, l = n2 - n1;
  const m = new THREE.Mesh(scaleUV(new THREE.PlaneGeometry(w, l), w / rep, l / rep), material);
  m.rotation.x = up ? -Math.PI / 2 : Math.PI / 2;
  m.position.set((x1 + x2) / 2, y, -(n1 + n2) / 2);
  scene.add(m);
  return m;
}

const FACING = { N: Math.PI, S: 0, E: Math.PI / 2, W: -Math.PI / 2 };
const OFFSET = { N: [0, 1], S: [0, -1], E: [1, 0], W: [-1, 0] };

// ---------------------------------------------------------------- tekstura chizish
function makeCanvas(w, h) {
  const c = document.createElement("canvas");
  c.width = w; c.height = h;
  return [c, c.getContext("2d")];
}
const SERIF = '"Cormorant Garamond", Georgia, serif';
const SANS = 'Inter, "Segoe UI", Arial, sans-serif';

// style: "dark" (to'q lavha), "ivory" (asar yorlig'i), "wall" (devorga yozilgan harflar)
function signTexture({ w = 1024, h = 256, title = "", sub = "", lines = [], style = "dark", accent = "#d4af37",
                       titleSize = 76, subSize = 36, lineSize = 40, align = "center" }) {
  const [c, g] = makeCanvas(w, h);
  const pal = {
    dark: { bg: "#0f121b", title: "#f1ebdd", sub: accent, line: "#d9d3c6" },
    ivory: { bg: "#f1ebdd", title: "#1e1b17", sub: "#6e5d3f", line: "#3a352d" },
    wall: { bg: null, title: "#f6efe0", sub: accent, line: "#f0e9da" },
  }[style];
  if (pal.bg) {
    g.fillStyle = pal.bg;
    g.fillRect(0, 0, w, h);
  }
  if (style === "dark") {
    const lw = Math.max(3, w / 220);
    g.strokeStyle = accent; g.lineWidth = lw;
    g.strokeRect(lw * 2.5, lw * 2.5, w - lw * 5, h - lw * 5);
  }
  if (style === "wall") { g.shadowColor = "rgba(0,0,0,0.55)"; g.shadowBlur = 10; g.shadowOffsetY = 3; }
  const fit = (text, max) => {
    let t = text || "";
    if (g.measureText(t).width <= max) return t;
    while (t.length > 2 && g.measureText(t + "…").width > max) t = t.slice(0, -1);
    return t + "…";
  };
  const pad = w * 0.07;
  g.textAlign = "center";
  g.fillStyle = pal.title;
  g.font = `600 ${titleSize}px ${SERIF}`;
  let y;
  if (!sub && !lines.length) {
    y = h / 2 + titleSize * 0.32;
    g.fillText(fit(title, w - 2 * pad), w / 2, y);
  } else {
    y = lines.length ? pad + titleSize * 0.95 : h / 2 - (subSize * 0.55) + titleSize * 0.18;
    g.fillText(fit(title, w - 2 * pad), w / 2, y);
    if (sub) {
      g.fillStyle = pal.sub;
      g.font = `500 ${subSize}px ${SANS}`;
      y += subSize * 1.55;
      g.fillText(fit(sub.toUpperCase(), w - 2 * pad), w / 2, y);
    }
  }
  if (lines.length) {
    g.textAlign = align;
    y += lineSize * 0.9;
    const x = align === "left" ? pad : w / 2;
    for (const ln of lines) {
      y += lineSize * 1.5;
      g.fillStyle = ln.color || pal.line;
      g.font = `500 ${lineSize}px ${SERIF}`;
      g.fillText(fit(ln.text, w - 2 * pad), x, y);
    }
  }
  return canvasTex(c);
}

// Devor tagidagi yumshoq soya (ambient occlusion o'rniga)
const aoTex = (() => {
  const [c, g] = makeCanvas(4, 64);
  const grad = g.createLinearGradient(0, 0, 0, 64);
  grad.addColorStop(0, "rgba(0,0,0,0.5)");
  grad.addColorStop(1, "rgba(0,0,0,0)");
  g.fillStyle = grad;
  g.fillRect(0, 0, 4, 64);
  return canvasTex(c);
})();
const aoMat = new THREE.MeshBasicMaterial({ map: aoTex, transparent: true, depthWrite: false });

// Asar ortidagi iliq yorug'lik dog'i
const glowTex = (() => {
  const [c, g] = makeCanvas(256, 256);
  const grad = g.createRadialGradient(128, 100, 8, 128, 128, 128);
  grad.addColorStop(0, "rgba(255,236,205,0.9)");
  grad.addColorStop(0.55, "rgba(255,222,180,0.28)");
  grad.addColorStop(1, "rgba(255,210,160,0)");
  g.fillStyle = grad;
  g.fillRect(0, 0, 256, 256);
  return canvasTex(c);
})();
const glowMat = new THREE.MeshBasicMaterial({
  map: glowTex, transparent: true, opacity: HIGH ? 0.18 : 0.38, blending: THREE.AdditiveBlending, depthWrite: false,
});

// Gilam: to'q qizil, oltin hoshiya
function runnerTex(base = "#5e1a22", border = "#c9a243") {
  const [c, g] = makeCanvas(256, 512);
  g.fillStyle = base; g.fillRect(0, 0, 256, 512);
  g.fillStyle = border; g.fillRect(14, 0, 8, 512); g.fillRect(234, 0, 8, 512);
  g.fillStyle = "rgba(201,162,67,0.35)"; g.fillRect(30, 0, 3, 512); g.fillRect(223, 0, 3, 512);
  g.strokeStyle = "rgba(201,162,67,0.22)"; g.lineWidth = 3;
  for (let y = 0; y < 512; y += 128) {
    g.beginPath(); g.moveTo(128, y + 14); g.lineTo(190, y + 64); g.lineTo(128, y + 114); g.lineTo(66, y + 64); g.closePath(); g.stroke();
  }
  for (let i = 0; i < 9000; i++) {   // tuk to'qimasi
    g.fillStyle = `rgba(0,0,0,${Math.random() * 0.08})`;
    g.fillRect(Math.random() * 256, Math.random() * 512, 2, 2);
  }
  return canvasTex(c, true);
}
const runnerMat = std({ map: runnerTex(), roughness: 1 });
function runner(x1, x2, n1, n2) {
  const w = x2 - x1, l = n2 - n1;
  const m = new THREE.Mesh(scaleUV(new THREE.PlaneGeometry(w, l), 1, l / (w * 2)), runnerMat);
  m.rotation.x = -Math.PI / 2;
  m.position.set((x1 + x2) / 2, 0.012, -(n1 + n2) / 2);
  scene.add(m);
}
function rug(x1, x2, n1, n2, accent) {
  const [c, g] = makeCanvas(512, 512);
  const hsl = {};
  new THREE.Color(accent).getHSL(hsl);
  const base = new THREE.Color().setHSL(hsl.h, 0.42, 0.13);
  g.fillStyle = "#" + base.getHexString(); g.fillRect(0, 0, 512, 512);
  g.strokeStyle = "#b8913f"; g.lineWidth = 10; g.strokeRect(22, 22, 468, 468);
  g.lineWidth = 3; g.globalAlpha = 0.6; g.strokeRect(44, 44, 424, 424);
  g.globalAlpha = 0.3;
  g.beginPath(); g.moveTo(256, 90); g.lineTo(422, 256); g.lineTo(256, 422); g.lineTo(90, 256); g.closePath(); g.stroke();
  g.globalAlpha = 1;
  for (let i = 0; i < 14000; i++) { g.fillStyle = `rgba(0,0,0,${Math.random() * 0.1})`; g.fillRect(Math.random() * 512, Math.random() * 512, 2, 2); }
  const m = new THREE.Mesh(new THREE.PlaneGeometry(x2 - x1, n2 - n1), std({ map: canvasTex(c), roughness: 1 }));
  m.rotation.x = -Math.PI / 2;
  m.position.set((x1 + x2) / 2, 0.012, -(n1 + n2) / 2);
  scene.add(m);
}

// Qizil baxmal gilam: to'q yoqut qizil, hashamatli oltin naqsh va hoshiya
function redCarpetTex() {
  const [c, g] = makeCanvas(512, 512);
  g.fillStyle = "#80081d"; g.fillRect(0, 0, 512, 512);
  g.fillStyle = "#d4af37";
  g.fillRect(16, 0, 10, 512); g.fillRect(486, 0, 10, 512);
  g.fillRect(32, 0, 4, 512); g.fillRect(476, 0, 4, 512);
  g.strokeStyle = "rgba(212,175,55,0.45)"; g.lineWidth = 4;
  for (let y = 0; y < 512; y += 128) {
    g.beginPath();
    g.arc(256, y + 64, 40, 0, Math.PI * 2);
    g.stroke();
    g.strokeRect(90, y + 18, 332, 92);
  }
  for (let i = 0; i < 15000; i++) {
    g.fillStyle = `rgba(0,0,0,${Math.random() * 0.08})`;
    g.fillRect(Math.random() * 512, Math.random() * 512, 2, 2);
  }
  return canvasTex(c, true);
}
const redCarpetMat = std({ map: redCarpetTex(), roughness: 0.95 });
function redRunner(x1, x2, n1, n2) {
  const minX = Math.min(x1, x2), maxX = Math.max(x1, x2);
  const minN = Math.min(n1, n2), maxN = Math.max(n1, n2);
  const w = maxX - minX, l = maxN - minN;
  const m = new THREE.Mesh(scaleUV(new THREE.PlaneGeometry(w, l), w > l ? w / 2 : 1, l >= w ? l / 2 : 1), redCarpetMat);
  m.rotation.x = -Math.PI / 2;
  m.position.set((minX + maxX) / 2, 0.015, -(minN + maxN) / 2);
  scene.add(m);
  return m;
}

// ---------------------------------------------------------------- devorlar
// To'g'ri devor, eshik o'rinlari bilan. Eshiklarga ramka (casing) qo'yiladi.
function wall(x1, n1, x2, n2, material, openings = []) {
  const horiz = n1 === n2;
  const a = horiz ? Math.min(x1, x2) : Math.min(n1, n2);
  const b = horiz ? Math.max(x1, x2) : Math.max(n1, n2);
  const fixed = horiz ? n1 : x1;
  const put = (c, cy, len, h, d, mat, solid, uv) => horiz
    ? box(c, cy, fixed, len, h, d, mat, solid, uv)
    : box(fixed, cy, c, d, h, len, mat, solid, uv);
  const seg = (s, e, y0, y1, solid) => {
    const len = e - s, h = y1 - y0;
    if (len < 0.02 || h < 0.02) return;
    put((s + e) / 2, (y0 + y1) / 2, len + (solid ? T : 0), h, T, material, solid, [len / 4, h / 4]);
  };
  let cur = a;
  for (const o of [...openings].sort((p, q) => p.c - q.c)) {
    const l = o.c - o.w / 2, r = o.c + o.w / 2, oh = o.h || DOOR_H;
    seg(cur, l, 0, H, true);
    seg(l, r, oh, H, false);
    if (o.casing !== false) {
      const cm = o.casing || M.wood, cd = T + 0.16;
      put(l - 0.1, (oh + 0.12) / 2, 0.2, oh + 0.12, cd, cm, false);
      put(r + 0.1, (oh + 0.12) / 2, 0.2, oh + 0.12, cd, cm, false);
      put(o.c, oh + 0.14, o.w + 0.5, 0.28, cd + 0.04, cm, false);
      put(o.c, oh + 0.3, o.w + 0.7, 0.06, cd + 0.08, M.gold, false);
    }
    cur = Math.max(cur, r);
  }
  seg(cur, b, 0, H, true);
}

// Xona ichki bezagi: plintus, panel (wainscot), karniz va pol bo'ylab soya.
// sides: {N|S|E|W: {gaps:[{c,w}], skip:bool}}
function trimRoom(room, sides, { panel, rail = M.wood, crown = M.ivory } = {}) {
  for (const side of ["N", "S", "E", "W"]) {
    const spec = sides[side] || {};
    if (spec.skip) continue;
    const alongX = side === "N" || side === "S";
    const fixed = { N: room.n2, S: room.n1, E: room.x2, W: room.x1 }[side];
    const inward = { N: -1, S: 1, E: -1, W: 1 }[side];
    const a = alongX ? room.x1 + T / 2 : room.n1 + T / 2;
    const b = alongX ? room.x2 - T / 2 : room.n2 - T / 2;
    const surface = fixed + inward * (T / 2);
    const segs = [];
    let cur = a;
    for (const gp of [...(spec.gaps || [])].sort((p, q) => p.c - q.c)) {
      const l = gp.c - gp.w / 2 - 0.2, r = gp.c + gp.w / 2 + 0.2;
      if (l > cur) segs.push([cur, l]);
      cur = Math.max(cur, r);
    }
    if (cur < b) segs.push([cur, b]);

    const strip = (s, e, y0, y1, depth, mat) => {
      const len = e - s, c = (s + e) / 2, off = surface + inward * depth / 2;
      if (alongX) box(c, (y0 + y1) / 2, off, len, y1 - y0, depth, mat);
      else box(off, (y0 + y1) / 2, c, depth, y1 - y0, len, mat);
    };
    for (const [s, e] of segs) {
      if (e - s < 0.05) continue;
      if (panel) strip(s, e, 0, RAIL, 0.025, panel);
      strip(s, e, 0, 0.2, 0.05, M.wood);
      strip(s, e, RAIL - 0.02, RAIL + 0.05, 0.06, rail);
      strip(s, e, H - 0.22, H, 0.14, crown);
      strip(s, e, H - 0.26, H - 0.22, 0.09, M.gold);
      // pol bo'ylab soya
      const ao = new THREE.Mesh(new THREE.PlaneGeometry(e - s, 0.9), aoMat);
      ao.rotation.x = -Math.PI / 2;
      if (alongX) {
        ao.position.set((s + e) / 2, 0.015, -(surface + inward * 0.45));
        ao.rotation.z = side === "N" ? 0 : Math.PI;
      } else {
        ao.position.set(surface + inward * 0.45, 0.015, -(s + e) / 2);
        ao.rotation.z = side === "E" ? -Math.PI / 2 : Math.PI / 2;
      }
      scene.add(ao);
    }
  }
}

// Devorga osiladigan tekis lavha
function panelSign(x, y, n, facing, w, h, texture, extra = 0) {
  const [ox, on] = OFFSET[facing];
  const d = T / 2 + 0.03 + extra;
  const m = new THREE.Mesh(new THREE.PlaneGeometry(w, h), new THREE.MeshBasicMaterial({ map: texture, transparent: true, color: 0xeeeeee }));
  m.position.set(x + ox * d, y, -(n + on * d));
  m.rotation.y = FACING[facing];
  scene.add(m);
  return m;
}

// ---------------------------------------------------------------- jihozlar
function downlight(x, n, r = 0.32) {
  const m = new THREE.Mesh(new THREE.CircleGeometry(r, 28), M.lamp);
  m.rotation.x = Math.PI / 2;
  m.position.set(x, H - 0.01, -n);
  scene.add(m);
  const ring = new THREE.Mesh(new THREE.TorusGeometry(r + 0.04, 0.03, 8, 28), M.gold);
  ring.rotation.x = Math.PI / 2;
  ring.position.set(x, H - 0.02, -n);
  scene.add(ring);
}

// Shiftdagi yorug'lik paneli (galereyalardagi diffuzor)
function lightPanel(x1, x2, n1, n2) {
  const [c, g] = makeCanvas(256, 256);
  const grad = g.createRadialGradient(128, 128, 20, 128, 128, 180);
  grad.addColorStop(0, "#fffaf0"); grad.addColorStop(1, "#efe4cf");
  g.fillStyle = grad; g.fillRect(0, 0, 256, 256);
  g.strokeStyle = "rgba(160,140,100,0.35)"; g.lineWidth = 3;
  for (let i = 0; i <= 256; i += 64) { g.beginPath(); g.moveTo(i, 0); g.lineTo(i, 256); g.stroke(); g.beginPath(); g.moveTo(0, i); g.lineTo(256, i); g.stroke(); }
  const t = canvasTex(c, true);
  t.repeat.set((x2 - x1) / 2.5, (n2 - n1) / 2.5);
  const m = new THREE.Mesh(new THREE.PlaneGeometry(x2 - x1, n2 - n1), new THREE.MeshBasicMaterial({ map: t }));
  m.rotation.x = Math.PI / 2;
  m.position.set((x1 + x2) / 2, H - 0.02, -(n1 + n2) / 2);
  scene.add(m);
  // ramka
  const cx = (x1 + x2) / 2, cn = (n1 + n2) / 2, w = x2 - x1, l = n2 - n1;
  box(cx, H - 0.06, n1, w + 0.2, 0.1, 0.1, M.ivory);
  box(cx, H - 0.06, n2, w + 0.2, 0.1, 0.1, M.ivory);
  box(x1, H - 0.06, cn, 0.1, 0.1, l, M.ivory);
  box(x2, H - 0.06, cn, 0.1, 0.1, l, M.ivory);
}

function bench(x, n, alongN, cushion) {
  const L = 2.8, W = 0.8;
  const sx = alongN ? W : L, sz = alongN ? L : W;
  box(x, 0.36, n, sx, 0.08, sz, M.wood);
  box(x, 0.47, n, sx - 0.06, 0.14, sz - 0.06, cushion);
  for (const dx of [-1, 1]) for (const dz of [-1, 1]) {
    box(x + dx * (sx / 2 - 0.08), 0.16, n + dz * (sz / 2 - 0.08), 0.06, 0.32, 0.06, M.gold);
  }
  addSolid(x - sx / 2, x + sx / 2, -n - sz / 2, -n + sz / 2);
}

const leafGeo = new THREE.IcosahedronGeometry(0.3, 0);
function plant(x, n) {
  const pot = new THREE.Mesh(new THREE.CylinderGeometry(0.3, 0.22, 0.6, 20), M.ceramic);
  pot.position.set(x, 0.3, -n);
  scene.add(pot);
  const rim = new THREE.Mesh(new THREE.TorusGeometry(0.3, 0.025, 6, 20), M.gold);
  rim.rotation.x = Math.PI / 2;
  rim.position.set(x, 0.6, -n);
  scene.add(rim);
  const seed = Math.abs(Math.sin(x * 12.9 + n * 78.2)) * 1000;
  for (let i = 0; i < 9; i++) {
    const a = i * 2.4 + seed, r = 0.12 + (i % 3) * 0.07;
    const leaf = new THREE.Mesh(leafGeo, M.leaf[i % 3]);
    leaf.position.set(x + Math.cos(a) * r, 0.95 + i * 0.12, -n + Math.sin(a) * r);
    leaf.scale.setScalar(1.15 - i * 0.07);
    leaf.rotation.set(a, a * 0.7, 0);
    scene.add(leaf);
  }
  addSolid(x - 0.35, x + 0.35, -n - 0.35, -n + 0.35);
}

// ---------------------------------------------------------------- atirgullar (Love Story)
const rosePetalGeo = new THREE.SphereGeometry(0.08, 7, 7);
const roseLeafGeo = new THREE.ConeGeometry(0.05, 0.15, 5);
const roseMat1 = std({ color: 0xc41230, roughness: 0.65 });
const roseMat2 = std({ color: 0x960018, roughness: 0.7 });
const roseMatPink = std({ color: 0xdf2b5e, roughness: 0.6 });
const roseLeafMat = std({ color: 0x1b4d24, roughness: 0.8 });

function roseVase(x, n, scale = 1.0) {
  const g = new THREE.Group();
  g.position.set(x, 0, -n);

  const stand = new THREE.Mesh(new THREE.CylinderGeometry(0.24 * scale, 0.28 * scale, 0.75 * scale, 20), M.gold);
  stand.position.y = 0.375 * scale;
  g.add(stand);

  const vase = new THREE.Mesh(new THREE.CylinderGeometry(0.18 * scale, 0.12 * scale, 0.45 * scale, 18), M.ivory);
  vase.position.y = (0.75 + 0.225) * scale;
  g.add(vase);
  const vaseLip = new THREE.Mesh(new THREE.TorusGeometry(0.18 * scale, 0.02 * scale, 6, 20), M.gold);
  vaseLip.rotation.x = Math.PI / 2;
  vaseLip.position.y = (0.75 + 0.45) * scale;
  g.add(vaseLip);

  const cy = (0.75 + 0.45) * scale;
  const roseMats = [roseMat1, roseMat2, roseMatPink];
  for (let i = 0; i < 18; i++) {
    const theta = i * 1.5;
    const rad = 0.03 + (i % 4) * 0.045 * scale;
    const rx = Math.cos(theta) * rad;
    const rz = Math.sin(theta) * rad;
    const ry = cy + 0.04 + (i % 3) * 0.045 * scale;

    const rGroup = new THREE.Group();
    rGroup.position.set(rx, ry, rz);
    const mat = roseMats[i % 3];
    for (let p = 0; p < 5; p++) {
      const pa = p * ((Math.PI * 2) / 5);
      const petal = new THREE.Mesh(rosePetalGeo, mat);
      petal.scale.set(0.7 * scale, 0.9 * scale, 0.5 * scale);
      petal.position.set(Math.cos(pa) * 0.025 * scale, 0, Math.sin(pa) * 0.025 * scale);
      petal.rotation.y = pa;
      rGroup.add(petal);
    }
    const centerPetal = new THREE.Mesh(rosePetalGeo, mat);
    centerPetal.scale.set(0.6 * scale, 1.0 * scale, 0.6 * scale);
    centerPetal.position.y = 0.02 * scale;
    rGroup.add(centerPetal);
    g.add(rGroup);
  }
  for (let i = 0; i < 8; i++) {
    const a = i * (Math.PI / 4);
    const lf = new THREE.Mesh(roseLeafGeo, roseLeafMat);
    lf.position.set(Math.cos(a) * 0.18 * scale, cy + 0.02, Math.sin(a) * 0.18 * scale);
    lf.rotation.z = Math.cos(a) * 0.8;
    lf.rotation.x = Math.sin(a) * 0.8;
    g.add(lf);
  }

  scene.add(g);
  addSolid(x - 0.32 * scale, x + 0.32 * scale, -n - 0.32 * scale, -n + 0.32 * scale);
}

function rosePetals(x1, x2, n1, n2, count = 140) {
  const petalGeo = new THREE.PlaneGeometry(0.06, 0.05);
  const mat = new THREE.MeshBasicMaterial({ color: 0xbd1836, side: THREE.DoubleSide });
  for (let i = 0; i < count; i++) {
    const px = x1 + Math.random() * (x2 - x1);
    const pn = n1 + Math.random() * (n2 - n1);
    const m = new THREE.Mesh(petalGeo, mat);
    m.rotation.x = -Math.PI / 2;
    m.rotation.z = Math.random() * Math.PI * 2;
    m.position.set(px, 0.016, -pn);
    scene.add(m);
  }
}

// ---------------------------------------------------------------- MARRY ME (Romantic Proposal)
const candleWaxMat = std({ color: 0xfffcf5, roughness: 0.35 });
const flameGeo = new THREE.ConeGeometry(0.02, 0.055, 6);
const flameMat = new THREE.MeshBasicMaterial({ color: 0xffc84a });
const glassMat = new THREE.MeshPhysicalMaterial({
  color: 0xffffff,
  transparent: true,
  opacity: 0.3,
  roughness: 0.08,
  transmission: 0.82,
});

function pillarCandle(x, n, height = 0.32, withGlass = true) {
  const g = new THREE.Group();
  g.position.set(x, 0, -n);

  const wax = new THREE.Mesh(new THREE.CylinderGeometry(0.04, 0.04, height, 12), candleWaxMat);
  wax.position.y = height / 2;
  g.add(wax);

  const flame = new THREE.Mesh(flameGeo, flameMat);
  flame.position.y = height + 0.028;
  g.add(flame);

  if (withGlass) {
    const gh = height + 0.15;
    const glass = new THREE.Mesh(new THREE.CylinderGeometry(0.072, 0.072, gh, 14, 1, true), glassMat);
    glass.position.y = gh / 2;
    g.add(glass);

    const rim = new THREE.Mesh(new THREE.TorusGeometry(0.072, 0.006, 4, 14), M.gold);
    rim.rotation.x = Math.PI / 2;
    rim.position.y = gh;
    g.add(rim);
  }

  scene.add(g);
  return g;
}

function candleAisle(startX, endX, zLeft, zRight, count = 10) {
  for (let i = 0; i <= count; i++) {
    const t = i / count;
    const x = startX + (endX - startX) * t;
    const h1 = 0.22 + (i % 3) * 0.08;
    const h2 = 0.25 + ((i + 1) % 3) * 0.08;
    pillarCandle(x, zLeft, h1, true);
    pillarCandle(x, zRight, h2, true);

    if (i % 3 === 0) {
      const cl = new THREE.PointLight(0xff9944, 1.4, 4.5, 1.4);
      cl.position.set(x, 0.45, -zLeft);
      scene.add(cl);

      const cr = new THREE.PointLight(0xff9944, 1.4, 4.5, 1.4);
      cr.position.set(x, 0.45, -zRight);
      scene.add(cr);
    }
  }
}

function heartCandlesAndRoses(cx, cz, radius = 1.4) {
  const steps = 30;
  for (let i = 0; i < steps; i++) {
    const t = (i / steps) * Math.PI * 2;
    const sinT = Math.sin(t);
    const hx = (16 * Math.pow(sinT, 3)) / 16;
    const hz = -(13 * Math.cos(t) - 5 * Math.cos(2 * t) - 2 * Math.cos(3 * t) - Math.cos(4 * t)) / 16;
    const px = cx + hx * radius;
    const pz = cz + hz * radius;

    pillarCandle(px, pz, 0.12 + (i % 2) * 0.05, false);

    for (let r = 0; r < 4; r++) {
      const petal = new THREE.Mesh(rosePetalGeo, (r % 2 === 0) ? roseMat1 : roseMat2);
      petal.scale.set(0.6, 0.2, 0.6);
      petal.position.set(px + (Math.random() - 0.5) * 0.12, 0.015, -pz + (Math.random() - 0.5) * 0.12);
      scene.add(petal);
    }
  }
  const heartGlow = new THREE.PointLight(0xff2a6d, 2.8, 6.0, 1.2);
  heartGlow.position.set(cx, 0.6, -cz);
  scene.add(heartGlow);
}

function buildMarryMeLetters(cx, n, facing, toRoomDir) {
  const letters = ["M", "A", "R", "R", "Y", " ", "M", "E"];
  const count = letters.length;
  const letterW = 0.52;
  const spacing = 0.64;
  const totalW = (count - 1) * spacing;
  const startN = n - totalW / 2;

  const rotY = (facing === "E") ? -Math.PI / 2 : Math.PI / 2;

  letters.forEach((char, idx) => {
    if (char === " ") return;
    const ln = startN + idx * spacing;
    const g = new THREE.Group();
    g.position.set(cx, 0.38, -ln);
    g.rotation.y = rotY;

    const [cv, ctx2] = makeCanvas(256, 320);
    ctx2.fillStyle = "#1e0b12";
    ctx2.fillRect(0, 0, 256, 320);

    ctx2.strokeStyle = "#ffd700";
    ctx2.lineWidth = 10;
    ctx2.strokeRect(10, 10, 236, 300);

    ctx2.fillStyle = "#ffffff";
    ctx2.font = "bold 210px 'Cinzel', 'Playfair Display', serif, sans-serif";
    ctx2.textAlign = "center";
    ctx2.textBaseline = "middle";
    ctx2.shadowColor = "#ffb07c";
    ctx2.shadowBlur = 24;
    ctx2.fillText(char, 128, 160);

    ctx2.fillStyle = "#fff4cc";
    const dots = [[30, 30], [128, 25], [226, 30], [226, 160], [226, 290], [128, 295], [30, 290], [30, 160]];
    for (const [dx, dy] of dots) {
      ctx2.beginPath();
      ctx2.arc(dx, dy, 9, 0, Math.PI * 2);
      ctx2.fill();
    }

    const mat = std({
      map: canvasTex(cv),
      emissive: 0xffebd0,
      emissiveIntensity: 0.95,
      roughness: 0.35,
    });

    const boxMesh = new THREE.Mesh(new THREE.BoxGeometry(letterW, 0.76, 0.12), mat);
    g.add(boxMesh);

    const baseMesh = new THREE.Mesh(new THREE.BoxGeometry(letterW + 0.08, 0.06, 0.18), M.gold);
    baseMesh.position.y = -0.38;
    g.add(baseMesh);

    scene.add(g);
  });

  const marqueeGlow = new THREE.PointLight(0xffca80, 3.2, 10, 1.2);
  marqueeGlow.position.set(cx + toRoomDir * 0.8, 0.9, -n);
  scene.add(marqueeGlow);
}

function floralProposalArch(x, cz, facing, toRoomDir) {
  const g = new THREE.Group();
  g.position.set(x, 0, -cz);
  g.rotation.y = (facing === "E") ? -Math.PI / 2 : Math.PI / 2;

  const archSpan = 4.2;
  const archHeight = 4.3;
  const pillarMat = M.gold;

  const pL = new THREE.Mesh(new THREE.CylinderGeometry(0.12, 0.16, archHeight - 1.2, 16), pillarMat);
  pL.position.set(-archSpan / 2, (archHeight - 1.2) / 2, 0);
  g.add(pL);

  const pR = new THREE.Mesh(new THREE.CylinderGeometry(0.12, 0.16, archHeight - 1.2, 16), pillarMat);
  pR.position.set(archSpan / 2, (archHeight - 1.2) / 2, 0);
  g.add(pR);

  const archCurve = new THREE.TorusGeometry(archSpan / 2, 0.14, 10, 30, Math.PI);
  const archMesh = new THREE.Mesh(archCurve, pillarMat);
  archMesh.position.set(0, archHeight - 1.2, 0);
  g.add(archMesh);

  const roseMats = [roseMat1, roseMat2, roseMatPink];
  const vineGeo = new THREE.SphereGeometry(0.14, 6, 6);
  const vineMat = std({ color: 0x16381b, roughness: 0.85 });

  for (let i = 0; i <= 36; i++) {
    const a = (i / 36) * Math.PI;
    const ax = Math.cos(a) * (archSpan / 2);
    const ay = (archHeight - 1.2) + Math.sin(a) * (archSpan / 2);

    const vg = new THREE.Mesh(vineGeo, vineMat);
    vg.scale.set(1.4, 0.9, 1.2);
    vg.position.set(ax, ay, (i % 2 === 0 ? 0.05 : -0.05));
    g.add(vg);

    const rMat = roseMats[i % 3];
    const rMesh = new THREE.Mesh(rosePetalGeo, rMat);
    rMesh.scale.set(1.3, 1.3, 1.3);
    rMesh.position.set(ax + (i % 3 - 1) * 0.08, ay, 0.12);
    g.add(rMesh);

    if (i % 2 === 0) {
      const fl = new THREE.Mesh(new THREE.SphereGeometry(0.025, 6, 6), new THREE.MeshBasicMaterial({ color: 0xfff0aa }));
      fl.position.set(ax, ay + 0.05, 0.14);
      g.add(fl);
    }
  }

  for (let y = 0.5; y < archHeight - 1.2; y += 0.35) {
    for (const px of [-archSpan / 2, archSpan / 2]) {
      const rMesh = new THREE.Mesh(rosePetalGeo, roseMats[Math.floor(y * 3) % 3]);
      rMesh.scale.set(1.2, 1.2, 1.2);
      rMesh.position.set(px + (px > 0 ? -0.05 : 0.05), y, 0.1);
      g.add(rMesh);

      const vg = new THREE.Mesh(vineGeo, vineMat);
      vg.scale.set(1.2, 0.7, 1.0);
      vg.position.set(px, y, 0);
      g.add(vg);
    }
  }

  scene.add(g);
  return g;
}

function proposalRingPedestal(x, cz, facing, toRoomDir) {
  const g = new THREE.Group();
  g.position.set(x, 0, -cz);
  g.rotation.y = (facing === "E") ? -Math.PI / 2 : Math.PI / 2;

  const ped = new THREE.Mesh(new THREE.CylinderGeometry(0.24, 0.28, 0.95, 24), M.gold);
  ped.position.y = 0.95 / 2;
  g.add(ped);

  const pedTop = new THREE.Mesh(new THREE.CylinderGeometry(0.32, 0.24, 0.06, 24), M.marble);
  pedTop.position.y = 0.95 + 0.03;
  g.add(pedTop);

  const boxBottom = new THREE.Mesh(new THREE.BoxGeometry(0.14, 0.06, 0.14), M.velvetRed);
  boxBottom.position.set(0, 0.98 + 0.03, 0);
  g.add(boxBottom);

  const boxLid = new THREE.Mesh(new THREE.BoxGeometry(0.14, 0.05, 0.14), M.velvetRed);
  boxLid.position.set(0, 0.98 + 0.12, -0.05);
  boxLid.rotation.x = -Math.PI / 3;
  g.add(boxLid);

  const ringBand = new THREE.Mesh(new THREE.TorusGeometry(0.026, 0.005, 8, 20), M.gold);
  ringBand.position.set(0, 1.05, 0);
  ringBand.rotation.x = Math.PI / 2;
  g.add(ringBand);

  const diamondGeo = new THREE.OctahedronGeometry(0.016, 1);
  const diamondMat = new THREE.MeshPhysicalMaterial({
    color: 0xffffff,
    emissive: 0x99ddff,
    emissiveIntensity: 0.8,
    roughness: 0.05,
    transmission: 0.9,
    reflectivity: 1.0,
  });
  const diamond = new THREE.Mesh(diamondGeo, diamondMat);
  diamond.position.set(0, 1.08, 0);
  g.add(diamond);

  const sparkle = new THREE.PointLight(0xaae4ff, 1.2, 2.5, 1.5);
  sparkle.position.set(0, 1.15, 0);
  g.add(sparkle);

  scene.add(g);
  addSolid(x - 0.35, x + 0.35, -cz - 0.35, -cz + 0.35);
}

function fairyLightsCanopy(x1, x2, n1, n2, y = 5.15) {
  const g = new THREE.Group();
  const lightGeo = new THREE.SphereGeometry(0.03, 5, 5);
  const lightMat = new THREE.MeshBasicMaterial({ color: 0xfff2cc });

  for (let z = n1 + 1.2; z <= n2 - 1.2; z += 1.4) {
    for (let x = x1 + 1.2; x <= x2 - 1.2; x += 1.4) {
      const sag = Math.sin(((x - x1) / (x2 - x1)) * Math.PI) * 0.28;
      const m = new THREE.Mesh(lightGeo, lightMat);
      m.position.set(x, y - sag, -z);
      g.add(m);
    }
  }
  scene.add(g);
}

function romanticWallDrapes(x1, x2, n, facingWall = "N") {
  const midX = (x1 + x2) / 2;
  for (const dx of [midX - 2.8, midX + 2.8]) {
    const drape = new THREE.Mesh(new THREE.BoxGeometry(1.6, 4.2, 0.12), M.velvetRed);
    drape.position.set(dx, 2.8, -n + (facingWall === "N" ? 0.06 : -0.06));
    scene.add(drape);

    const tieback = new THREE.Mesh(new THREE.BoxGeometry(1.7, 0.12, 0.16), M.gold);
    tieback.position.set(dx, 2.3, -n + (facingWall === "N" ? 0.08 : -0.08));
    scene.add(tieback);

    const sconce = new THREE.Mesh(new THREE.CylinderGeometry(0.18, 0.12, 0.08, 16), M.gold);
    sconce.position.set(dx, 2.8, -n + (facingWall === "N" ? 0.2 : -0.2));
    scene.add(sconce);

    pillarCandle(dx - 0.08, n + (facingWall === "N" ? 0.2 : -0.2), 0.24, false);
    pillarCandle(dx + 0.08, n + (facingWall === "N" ? 0.2 : -0.2), 0.24, false);

    const scLight = new THREE.PointLight(0xff9e4a, 1.2, 4.5, 1.4);
    scLight.position.set(dx, 3.1, -n + (facingWall === "N" ? 0.35 : -0.35));
    scene.add(scLight);
  }
}

function column(x, n) {
  const shaft = new THREE.Mesh(scaleUV(new THREE.CylinderGeometry(0.38, 0.44, H - 0.9, 28), 2, 2),
    std({ map: tex("column"), color: 0xf3eee4, roughness: 0.35 }));
  shaft.position.set(x, H / 2, -n);
  scene.add(shaft);
  box(x, 0.18, n, 1.15, 0.36, 1.15, M.marble, true);
  box(x, H - 0.2, n, 1.15, 0.3, 1.15, M.ivory);
  for (const [y, r] of [[0.45, 0.48], [H - 0.42, 0.44]]) {
    const ring = new THREE.Mesh(new THREE.TorusGeometry(r, 0.06, 10, 28), M.gold);
    ring.rotation.x = Math.PI / 2;
    ring.position.set(x, y, -n);
    scene.add(ring);
  }
}

// Velvet arqonli to'siq (stanchion)
const postGeo = new THREE.CylinderGeometry(0.03, 0.035, 0.9, 12);
const postBase = new THREE.CylinderGeometry(0.13, 0.15, 0.04, 20);
const postTop = new THREE.SphereGeometry(0.055, 14, 10);
function stanchion(g, x, z) {
  const p = new THREE.Mesh(postGeo, M.brass); p.position.set(x, 0.45 - PY, z); g.add(p);
  const b = new THREE.Mesh(postBase, M.brass); b.position.set(x, 0.02 - PY, z); g.add(b);
  const t = new THREE.Mesh(postTop, M.brass); t.position.set(x, 0.92 - PY, z); g.add(t);
}

// ---------------------------------------------------------------- asar osish
function hang(p, x, n, facing, accent) {
  const aspect = (p.w || 1) / (p.h || 1);
  let ph = PMAX_H, pw = ph * aspect;
  if (pw > PMAX_W) { pw = PMAX_W; ph = pw / aspect; }
  const fund = !p.owner;

  const [ox, on] = OFFSET[facing];
  const g = new THREE.Group();
  g.position.set(x + ox * (T / 2), PY, -(n + on * (T / 2)));
  g.rotation.y = FACING[facing];
  scene.add(g);

  const glow = new THREE.Mesh(new THREE.PlaneGeometry(pw + 2.4, ph + 2.6), glowMat);
  glow.position.set(0, 0.3, 0.004);
  g.add(glow);

  let face;
  if (fund) {     // muzey fondi — zarhal ramka
    const add = (w, h, d, z, mat) => { const m = new THREE.Mesh(new THREE.BoxGeometry(w, h, d), mat); m.position.z = z; g.add(m); };
    add(pw + 0.44, ph + 0.44, 0.08, 0.04, M.gold);
    add(pw + 0.3, ph + 0.3, 0.1, 0.05, M.wood);
    add(pw + 0.1, ph + 0.1, 0.11, 0.055, M.gold);
    face = 0.112;
  } else {        // foydalanuvchi asari — zamonaviy qora ramka + oq paspartu
    const fr = new THREE.Mesh(new THREE.BoxGeometry(pw + 0.56, ph + 0.56, 0.06), M.black);
    fr.position.z = 0.03; g.add(fr);
    const mb = new THREE.Mesh(new THREE.PlaneGeometry(pw + 0.42, ph + 0.42), M.matboard);
    mb.position.z = 0.061; g.add(mb);
    face = 0.063;
  }

  const t = loader.load(p.image);
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = aniso;
  const artMat = HIGH
    ? std({ map: t, emissive: 0xffffff, emissiveMap: t, emissiveIntensity: 0.55, roughness: 0.8 })
    : new THREE.MeshBasicMaterial({ map: t });
  const art = new THREE.Mesh(new THREE.PlaneGeometry(pw, ph), artMat);
  art.position.z = face;
  art.userData.painting = p;
  g.add(art);
  clickable.push(art);

  // muzey uslubidagi fil suyagi rangli yorliq
  const label = new THREE.Mesh(new THREE.PlaneGeometry(1.0, 0.25), new THREE.MeshBasicMaterial({
    map: signTexture({ w: 768, h: 192, style: "ivory", title: p.title,
      sub: p.artist + (p.year ? "  ·  " + p.year : ""), titleSize: 58, subSize: 28 }),
    color: 0xe6e2da,
  }));
  label.position.set(0, -ph / 2 - (fund ? 0.22 : 0.28) - 0.17, 0.02);
  g.add(label);

  if (p.mine) {   // foydalanuvchining o'z asari
    const badge = new THREE.Mesh(new THREE.PlaneGeometry(1.0, 0.22), new THREE.MeshBasicMaterial({
      map: signTexture({ w: 512, h: 112, title: "Sizning asaringiz", titleSize: 46, style: "dark", accent: "#5fbf8f" }),
    }));
    badge.position.set(0, ph / 2 + 0.48, 0.02);
    g.add(badge);
  }

  // to'siq: ikki ustun va velvet arqon
  const half = Math.max(pw / 2 + 0.35, 0.9), dz = 1.0;
  stanchion(g, -half, dz);
  stanchion(g, half, dz);
  const curve = new THREE.QuadraticBezierCurve3(
    new THREE.Vector3(-half, 0.86 - PY, dz), new THREE.Vector3(0, 0.62 - PY, dz), new THREE.Vector3(half, 0.86 - PY, dz));
  g.add(new THREE.Mesh(new THREE.TubeGeometry(curve, 16, 0.022, 6), M.velvetRed));
  g.updateMatrixWorld(true);
  const a = g.localToWorld(new THREE.Vector3(-half, 0, dz)), b = g.localToWorld(new THREE.Vector3(half, 0, dz));
  addSolid(Math.min(a.x, b.x) - 0.08, Math.max(a.x, b.x) + 0.08, Math.min(a.z, b.z) - 0.08, Math.max(a.z, b.z) + 0.08);

  spotFixture(g, new THREE.Vector3(0, H - 0.12 - PY, 1.8), new THREE.Vector3(0, -0.1, 0));
}

// Shiftdagi spot chiroq korpusi (g — asar guruhi, koordinatalar uning ichida)
function spotFixture(g, localPos, localTarget) {
  g.updateMatrixWorld(true);
  const fixturePos = g.localToWorld(localPos.clone());
  const target = g.localToWorld(localTarget.clone());
  const fx = new THREE.Group();
  fx.position.copy(fixturePos);
  const body = new THREE.Mesh(new THREE.CylinderGeometry(0.07, 0.1, 0.26, 14), M.black);
  body.rotation.x = Math.PI / 2;
  fx.add(body);
  const lens = new THREE.Mesh(new THREE.CircleGeometry(0.075, 14), M.lamp);
  lens.position.z = 0.131;
  fx.add(lens);
  scene.add(fx);
  fx.lookAt(target);
  const stem = new THREE.Mesh(new THREE.CylinderGeometry(0.015, 0.015, 0.12, 6), M.black);
  stem.position.set(fixturePos.x, H - 0.06, fixturePos.z);
  scene.add(stem);
  litSpots.push({ pos: fixturePos, target });
}

// ---------------------------------------------------------------- bo'sh o'rin
// Bosilsa — shu o'ringa rasm qo'yish sahifasi ochiladi.
function slotInfo(hall, slot) {
  const isReadOnly = Boolean(DATA.read_only);
  return {
    empty: true, slot, hallName: hall.name, sculpt: hall.kind === "sculptures",
    replace: isReadOnly ? null : `/my/art/new/?section=${encodeURIComponent(hall.slug)}&slot=${slot}`,
  };
}
function emptyTexture(slot, accent, sculpt) {
  const [c, g] = makeCanvas(512, 384);
  g.fillStyle = "rgba(12,14,20,0.6)";
  g.fillRect(0, 0, 512, 384);
  g.setLineDash([22, 14]);
  g.strokeStyle = accent; g.lineWidth = 6;
  g.strokeRect(14, 14, 484, 356);
  g.setLineDash([]);
  g.textAlign = "center";
  g.fillStyle = accent;
  g.font = `300 120px ${SERIF}`;
  g.fillText("+", 256, 170);
  g.fillStyle = "#f1ebdd";
  g.font = `600 52px ${SERIF}`;
  g.fillText(`${slot}-o'rin bo'sh`, 256, 245);
  g.fillStyle = "#cfc8b8";
  g.font = `500 26px ${SANS}`;
  g.fillText(sculpt ? "Haykal qo'yish uchun bosing" : "Rasm qo'yish uchun bosing", 256, 305);
  return canvasTex(c);
}
function emptyFrame(hall, slot, x, n, facing) {
  const [ox, on] = OFFSET[facing];
  const m = new THREE.Mesh(new THREE.PlaneGeometry(1.6, 1.2),
    new THREE.MeshBasicMaterial({ map: emptyTexture(slot, hall.accent, false), transparent: true }));
  m.position.set(x + ox * (T / 2 + 0.02), PY, -(n + on * (T / 2 + 0.02)));
  m.rotation.y = FACING[facing];
  m.userData.painting = slotInfo(hall, slot);
  scene.add(m);
  clickable.push(m);
}

// ---------------------------------------------------------------- haykallar
M.pedestal = std({ map: tex("floor_marble"), color: 0x2c303b, roughness: 0.2 });
// Choksiz haykal marmari: oq-sut rang, mayin dog'lar va ingichka kulrang tomirlar
function marbleTex() {
  const S = 512;
  const [c, g] = makeCanvas(S, S);
  g.fillStyle = "#efe9df";
  g.fillRect(0, 0, S, S);
  for (let i = 0; i < 60; i++) {   // mayin bulutsimon dog'lar
    const x = Math.random() * S, y = Math.random() * S, r = 30 + Math.random() * 90;
    const grad = g.createRadialGradient(x, y, 0, x, y, r);
    const tone = Math.random() < 0.5 ? "255,252,246" : "214,207,196";
    grad.addColorStop(0, `rgba(${tone},0.22)`);
    grad.addColorStop(1, `rgba(${tone},0)`);
    g.fillStyle = grad;
    g.fillRect(x - r, y - r, 2 * r, 2 * r);
  }
  g.lineCap = "round";
  for (let i = 0; i < 9; i++) {   // tomirlar (chetdan chiqib ketganlari qarama-qarshi tomonda takrorlanadi)
    let x = Math.random() * S, y = Math.random() * S, a = Math.random() * Math.PI * 2;
    g.strokeStyle = `rgba(120,112,104,${0.12 + Math.random() * 0.18})`;
    g.lineWidth = 0.6 + Math.random() * 1.6;
    for (let k = 0; k < 140; k++) {
      const nx = x + Math.cos(a) * 4, ny = y + Math.sin(a) * 4;
      for (const [dx, dy] of [[0, 0], [-S, 0], [S, 0], [0, -S], [0, S]]) {
        g.beginPath(); g.moveTo(x + dx, y + dy); g.lineTo(nx + dx, ny + dy); g.stroke();
      }
      x = (nx + S) % S; y = (ny + S) % S;
      a += (Math.random() - 0.5) * 0.5;
    }
  }
  return canvasTex(c, true);
}
const sculptMat = std({ map: marbleTex(), color: 0xfaf6ee, roughness: 0.24, metalness: 0.03 });

function pedestal(x, n) {
  box(x, 0.04, n, 1.24, 0.08, 1.24, M.gold);
  box(x, PED_H / 2, n, 1.0, PED_H - 0.08, 1.0, M.pedestal, true);
  box(x, PED_H - 0.03, n, 1.14, 0.06, 1.14, M.ivory);
}

// Postament old tomonidagi yorliq
function plaque(x, n, facing, texture, y = 0.55) {
  const [ox, on] = OFFSET[facing];
  const m = new THREE.Mesh(new THREE.PlaneGeometry(0.86, 0.215), new THREE.MeshBasicMaterial({ map: texture, color: 0xe6e2da }));
  m.position.set(x + ox * 0.505, y, -(n + on * 0.505));
  m.rotation.y = FACING[facing];
  scene.add(m);
  return m;
}

// Chuqurlik xaritasidan (kulrang = balandlik, alfa = shakl) yopiq 3D relyef-haykal:
// old yuza + orqa yuza + chetlarini tutashtiruvchi yon devorlar.
function sculptGeometry(img) {
  const w = img.width, h = img.height;
  const [, g] = makeCanvas(w, h);
  g.drawImage(img, 0, 0);
  const d = g.getImageData(0, 0, w, h).data;
  const step = 1;
  const gw = w, gh = h;
  const aspect = w / h;
  let sh = SMAX_H, sw = sh * aspect;
  if (sw > SMAX_W) { sw = SMAX_W; sh = sw / aspect; }
  const thick = Math.min(sw, sh) * 0.42; // qalinroq — odamga o'xshash hajm
  const N = gw * gh;
  const pos = new Float32Array(N * 6), uv = new Float32Array(N * 4), inside = new Uint8Array(N);
  for (let j = 0; j < gh; j++) {
    for (let i = 0; i < gw; i++) {
      const v = j * gw + i;
      const k = (j * w + i) * 4;
      // 16-bit balandlik: yuqori bayt (R) va quyi bayt (G)
      const hgt = (d[k] * 256 + d[k + 1]) / 65535;
      const backHgt = d[k + 2] / 255;
      inside[v] = d[k + 3] >= 128 ? 1 : 0;
      const x = (i / (gw - 1) - 0.5) * sw, y = (1 - j / (gh - 1)) * sh;
      pos.set([x, y, hgt * thick * 0.72], v * 3);   // old sirt — chuqurroq relyef
      pos.set([x, y, -backHgt * thick * 0.52], (N + v) * 3); // orqa sirt — qalinroq
      const u = i / (gw - 1), vCoord = 1 - j / (gh - 1);
      uv.set([u, vCoord], v * 2);
      uv.set([x / sw * 1.5, y / sh * 1.5], (N + v) * 2);
    }
  }
  const frontIdx = [];
  const backIdx = [];
  const sideIdx = [];
  const edges = new Set();
  const addTri = (a, b, c) => {
    frontIdx.push(a, b, c);
    backIdx.push(N + a, N + c, N + b);
    edges.add(a * N + b); edges.add(b * N + c); edges.add(c * N + a);
  };
  for (let j = 0; j < gh - 1; j++) {
    for (let i = 0; i < gw - 1; i++) {
      const a = j * gw + i, b = a + 1, c = a + gw, e = c + 1;
      if (inside[a] && inside[c] && inside[b]) addTri(a, c, b);
      if (inside[b] && inside[c] && inside[e]) addTri(b, c, e);
    }
  }
  // chegaradagi qirralar (faqat bitta uchburchakka tegishli) — yon devor
  for (const key of edges) {
    const u = Math.floor(key / N), v = key - u * N;
    if (edges.has(v * N + u)) continue;
    sideIdx.push(u, N + v, v, u, N + u, N + v);
  }
  const allIdx = [...frontIdx, ...backIdx, ...sideIdx];
  const geo = new THREE.BufferGeometry();
  geo.setAttribute("position", new THREE.BufferAttribute(pos, 3));
  geo.setAttribute("uv", new THREE.BufferAttribute(uv, 2));
  geo.setIndex(allIdx);
  geo.addGroup(0, frontIdx.length, 0); // Material 0: old yuza (marmar portret)
  geo.addGroup(frontIdx.length, backIdx.length + sideIdx.length, 1); // Material 1: orqa va yon devorlar (silliq marmar)
  geo.computeVertexNormals();

  // 16-bit chuqurlikdan per-pixel yuqori aniqlikdagi Normal Map yasash:
  const [nc, ng] = makeCanvas(w, h);
  const nImg = ng.createImageData(w, h);
  const nd = nImg.data;
  const getH = (px, py) => {
    const cx = Math.max(0, Math.min(w - 1, px));
    const cy = Math.max(0, Math.min(h - 1, py));
    const p = (cy * w + cx) * 4;
    return (d[p] * 256 + d[p + 1]) / 65535;
  };
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const idx = (y * w + x) * 4;
      if (d[idx + 3] < 64) {
        nd[idx] = 128; nd[idx + 1] = 128; nd[idx + 2] = 255; nd[idx + 3] = 0;
        continue;
      }
      const dx = (getH(x + 1, y) - getH(x - 1, y)) * 4.5;  // kuchli normal xarita — aniq detallar
      const dy = (getH(x, y + 1) - getH(x, y - 1)) * 4.5;
      let nx = -dx, ny = -dy, nz = 1.0;
      const len = Math.sqrt(nx * nx + ny * ny + nz * nz) || 1;
      nd[idx] = Math.round((nx / len * 0.5 + 0.5) * 255);
      nd[idx + 1] = Math.round((-ny / len * 0.5 + 0.5) * 255);
      nd[idx + 2] = Math.round((nz / len * 0.5 + 0.5) * 255);
      nd[idx + 3] = 255;
    }
  }
  ng.putImageData(nImg, 0, 0);
  const normalTex = normalCanvasTex(nc);

  return { geo, sw, sh, normalTex };
}

const imgLoader = new THREE.ImageLoader();
function placeSculpture(p, hall, x, n, facing) {
  pedestal(x, n);
  plaque(x, n, facing, signTexture({ w: 768, h: 192, style: "ivory", title: p.title,
    sub: p.artist + (p.year ? "  ·  " + p.year : ""), titleSize: 58, subSize: 28 }));
  const g = new THREE.Group();
  g.position.set(x, PED_H, -n);
  g.rotation.y = FACING[facing];
  scene.add(g);
  spotFixture(g, new THREE.Vector3(0, H - 0.12 - PED_H, 2.2), new THREE.Vector3(0, 0.9, 0));

  const badge = (top) => {
    if (!p.mine) return;
    const b = new THREE.Mesh(new THREE.PlaneGeometry(1.0, 0.22), new THREE.MeshBasicMaterial({
      map: signTexture({ w: 512, h: 112, title: "Sizning haykalingiz", titleSize: 44, style: "dark", accent: "#5fbf8f" }),
      side: THREE.DoubleSide,
    }));
    b.position.set(0, top + 0.3, 0);
    g.add(b);
  };

  if (p.depth) {
    imgLoader.load(p.depth, (img) => {
      const { geo, sh, normalTex } = sculptGeometry(img);
      const sculptTex = loader.load(p.image);
      sculptTex.colorSpace = THREE.SRGBColorSpace;
      sculptTex.anisotropy = aniso;
      const frontMat = std({
        map: sculptTex,
        normalMap: normalTex,
        normalScale: new THREE.Vector2(1.4, 1.4), // kuchaytirilgan normal — detallar aniqroq
        roughness: 0.28,   // silliqroq marmar effekti
        metalness: 0.01,
        envMapIntensity: 1.2,  // muhit aksini kuchaytirish
      });
      const backMat = std({
        map: marbleTex(),
        roughness: 0.30,
        metalness: 0.03,
        color: 0xeae4d8,
        envMapIntensity: 0.9,
      });
      const m = new THREE.Mesh(geo, [frontMat, backMat]);
      m.userData.painting = p;
      g.add(m);
      clickable.push(m);
      badge(sh);
    });
    return;
  }
  // haykal hali tayyor emas — rasm postament ustida tik turgan lavha sifatida
  const aspect = (p.w || 1) / (p.h || 1);
  let ph = 1.4, pw = ph * aspect;
  if (pw > 1.2) { pw = 1.2; ph = pw / aspect; }
  const frame = new THREE.Mesh(new THREE.BoxGeometry(pw + 0.12, ph + 0.12, 0.06), M.black);
  frame.position.set(0, ph / 2 + 0.06, 0);
  g.add(frame);
  const t = loader.load(p.image);
  t.colorSpace = THREE.SRGBColorSpace;
  const art = new THREE.Mesh(new THREE.PlaneGeometry(pw, ph), new THREE.MeshBasicMaterial({ map: t }));
  art.position.set(0, ph / 2 + 0.06, 0.031);
  art.userData.painting = p;
  g.add(art);
  clickable.push(art);
  badge(ph + 0.12);
}

function emptyPedestal(hall, slot, x, n, facing) {
  pedestal(x, n);
  // ikki tomonlama belgi: orqa tomondan ham yozuv to'g'ri o'qiladi
  const mat = new THREE.MeshBasicMaterial({ map: emptyTexture(slot, hall.accent, true), transparent: true });
  for (const turn of [0, Math.PI]) {
    const m = new THREE.Mesh(new THREE.PlaneGeometry(0.9, 0.675), mat);
    m.position.set(x, PED_H + 0.45, -n);
    m.rotation.y = FACING[facing] + turn;
    m.userData.painting = slotInfo(hall, slot);
    scene.add(m);
    clickable.push(m);
  }
}

// ---------------------------------------------------------------- zal geometriyasi
// 8 ta o'rin. Rasmlar zali: kirishdan qaraganda chap devor 3, ro'para devor 2, o'ng devor 3.
function paintingSlots(side, n0) {
  const inner = side * CW / 2, outer = side * (CW / 2 + D);
  const xa = Math.min(inner, outer), xb = Math.max(inner, outer);
  const along = (k) => Array.from({ length: k }, (_, j) => xa + 1 + (j + 0.5) * (xb - xa - 2) / k)
    .sort((p, q) => Math.abs(p - inner) - Math.abs(q - inner));
  const toRoom = side < 0 ? "E" : "W";
  return [
    ...along(3).map((x) => ({ x, n: n0, facing: "N" })),
    { x: outer, n: n0 + 2.8, facing: toRoom },
    { x: outer, n: n0 + HL - 2.8, facing: toRoom },
    ...along(3).reverse().map((x) => ({ x, n: n0 + HL, facing: "S" })),
  ];
}
// Haykallar zali: 6 ta postament, devor yaqinida (lekin devorga yopishtirilmagan, 1.8 birlik oraliq).
// Har bir haykal xona ichkarisiga — zalning markaziga qaraydi.
function sculptureSlots(side, n0) {
  const inner = side * CW / 2, outer = side * (CW / 2 + D);
  const xa = Math.min(inner, outer), xb = Math.max(inner, outer);
  // Devordan 1.8 birlik masofa saqlagan holda devor yaqiniga joylash
  const wallOffset = 1.8;
  const out = [];
  // Chap devor bo'ylab 2 ta haykal
  const leftX = xa + wallOffset;
  for (let k = 0; k < 2; k++) {
    out.push({ x: leftX, n: n0 + 3.5 + k * (HL - 7.0), facing: side < 0 ? "W" : "E" });
  }
  // Ro'para (tashqi) devor bo'ylab 2 ta haykal
  const farFacing = "S";
  const nearFacing = "N";
  for (let k = 0; k < 2; k++) {
    const wallN = k === 0 ? n0 + wallOffset : n0 + HL - wallOffset;
    const fac = k === 0 ? nearFacing : farFacing;
    out.push({ x: (xa + xb) / 2, n: wallN, facing: fac });
  }
  // O'ng devor bo'ylab 2 ta haykal
  const rightX = xb - wallOffset;
  for (let k = 0; k < 2; k++) {
    out.push({ x: rightX, n: n0 + 3.5 + k * (HL - 7.0), facing: side < 0 ? "E" : "W" });
  }
  return out;
}

// Marry Me zali: faqat 1 ta maxsus markaziy asar (eshik ro'parasidagi markaziy devorda).
function marryMeSlots(side, n0) {
  const outer = side * (CW / 2 + D);
  const c = n0 + HL / 2;
  const toRoom = side < 0 ? "E" : "W";
  return [
    { x: outer, n: c, facing: toRoom },
  ];
}

// Love Story zali: faqat 3 ta saralangan asar.
// 1-asar: chap devorda, 2-asar: eshik ro'parasidagi asosiy devorda, 3-asar: o'ng devorda.
function loveSlots(side, n0) {
  const inner = side * CW / 2, outer = side * (CW / 2 + D);
  const xa = Math.min(inner, outer), xb = Math.max(inner, outer);
  const cx = (xa + xb) / 2;
  const c = n0 + HL / 2;
  const toRoom = side < 0 ? "E" : "W";
  return [
    { x: cx, n: n0, facing: "N" },
    { x: outer, n: c, facing: toRoom },
    { x: cx, n: n0 + HL, facing: "S" },
  ];
}

// Zal rangi: aksent rangining to'q, xira varianti (klassik galereya devorlari)
function hallTones(accent) {
  const hsl = {};
  new THREE.Color(accent).getHSL(hsl);
  const wallC = new THREE.Color().setHSL(hsl.h, 0.32, 0.4);
  const panelC = new THREE.Color().setHSL(hsl.h, 0.3, 0.2);
  const cushionC = new THREE.Color().setHSL(hsl.h, 0.5, 0.17);
  return { wall: "#" + wallC.getHexString(), panel: "#" + panelC.getHexString(), cushion: "#" + cushionC.getHexString() };
}

function buildHall(hall, side, n0) {
  const inner = side * CW / 2, outer = side * (CW / 2 + D);
  const xa = Math.min(inner, outer), xb = Math.max(inner, outer);
  const c = n0 + HL / 2, cx = (xa + xb) / 2;
  const tones = hallTones(hall.accent);
  const wm = wallMat("wall_light", tones.wall);
  const sculpt = hall.kind === "sculptures";
  const isLove = hall.kind === "love" || hall.slug === "love";
  const isMarryMe = hall.kind === "marry_me" || hall.slug === "marry_me" || hall.slug === "uzbekistan";

  const fl = flat(xa, xb, n0, n0 + HL, floorMat(isMarryMe ? "floor_marble" : hall.floor), 0, true);
  fl.receiveShadow = false;
  // pol hoshiyasi
  const inlay = floorMat(isMarryMe ? "gold" : "floor_dark");
  flat(xa + 0.9, xb - 0.9, n0 + 0.9, n0 + 1.2, inlay, 0.004, true, 1);
  flat(xa + 0.9, xb - 0.9, n0 + HL - 1.2, n0 + HL - 0.9, inlay, 0.004, true, 1);
  flat(xa + 0.9, xa + 1.2, n0 + 1.2, n0 + HL - 1.2, inlay, 0.004, true, 1);
  flat(xb - 1.2, xb - 0.9, n0 + 1.2, n0 + HL - 1.2, inlay, 0.004, true, 1);

  flat(xa, xb, n0, n0 + HL, M.ceiling, H, false, 4);
  lightPanel(xa + 3.2, xb - 3.2, n0 + 3, n0 + HL - 3);

  if (isMarryMe) {
    // ------------------------------------
    // MARRY ME ZALI (Romantik taklif maskani)
    // ------------------------------------
    const toRoomDir = side < 0 ? -1 : 1;
    const farX = outer - toRoomDir * 1.5;

    // 1. Qizil darashkalar (Grand Red Carpet Runners)
    // Asosiy qizil darashka: eshikdan to'g'ri to'rga - nikoh taklifi sahnasigacha
    redRunner(inner, farX + toRoomDir * 0.4, c - 1.35, c + 1.35);

    // Kesishtirilgan hashamatli qizil darashkalar
    redRunner(cx - 1.35, cx + 1.35, n0 + 1.4, n0 + HL - 1.4);
    redRunner(xa + 1.4, xb - 1.4, n0 + 1.8, n0 + 3.4);
    redRunner(xa + 1.4, xb - 1.4, n0 + HL - 3.4, n0 + HL - 1.8);

    // 2. Qizil baxmal gilamlar (Plush Royal Red Velvet Carpets)
    // Markaziy katta qizil gilam
    rug(cx - 3.2, cx + 3.2, c - 3.2, c + 3.2, "#850a1e");
    // Nikoh taklifi sahnasi gilami
    rug(outer - toRoomDir * 4.6, outer - toRoomDir * 0.6, c - 3.2, c + 3.2, "#6d0618");

    // 3. Qizil darashka bo'ylab yonib turuvchi fonuslar va shamchalar yo'lagi
    candleAisle(inner + toRoomDir * 0.5, farX - toRoomDir * 0.8, c - 1.55, c + 1.55, 11);

    // 4. Pol ustida shamchalar va atirgullardan yasalgan romantik yurak
    const heartX = cx + (farX - cx) * 0.35;
    heartCandlesAndRoses(heartX, c, 1.4);

    // 5. Hashamatli Qizil Atirgulli Taklif Arkasi (Floral Proposal Arch)
    const archX = outer - toRoomDir * 1.0;
    floralProposalArch(archX, c, side < 0 ? "E" : "W", toRoomDir);

    // 6. 3D Yonib turuvchi "MARRY ME" Marquee harflari
    const marqueeX = archX - toRoomDir * 0.85;
    buildMarryMeLetters(marqueeX, c, side < 0 ? "E" : "W", toRoomDir);

    // 7. Olmos uzukli va baxmal qutichali taklif postamenti 💍
    const ringX = heartX + toRoomDir * 0.75;
    proposalRingPedestal(ringX, c, side < 0 ? "E" : "W", toRoomDir);

    // 8. Qizil atirgullar va gulli guldonlar (Rose Vases & Urns)
    const doorX = inner + toRoomDir * 1.0;
    roseVase(doorX, c - 2.2, 1.25);
    roseVase(doorX, c + 2.2, 1.25);

    // 4 ta burchakda katta qizil atirgul vazalari
    const cornerInX = inner + toRoomDir * 2.2;
    const cornerOutX = outer - toRoomDir * 2.2;
    roseVase(cornerInX, n0 + 1.8, 1.15);
    roseVase(cornerInX, n0 + HL - 1.8, 1.15);
    roseVase(cornerOutX, n0 + 1.8, 1.15);
    roseVase(cornerOutX, n0 + HL - 1.8, 1.15);

    // Sahna yonlarida hashamatli vazalar
    roseVase(farX, c - 3.2, 1.35);
    roseVase(farX, c + 3.2, 1.35);

    // 9. To'kilgan qizil atirgul barglari (Hundreds of dense rose petals)
    rosePetals(xa + 0.8, xb - 0.8, n0 + 0.8, n0 + HL - 0.8, 480);

    // 10. Shiftdagi yulduzli chiroqlar marjoni (Fairy light canopy)
    fairyLightsCanopy(xa, xb, n0, n0 + HL, 5.15);

    // 11. Yon devorlarda romantik pardalar va devor shamdonlari
    romanticWallDrapes(xa, xb, n0, "N");
    romanticWallDrapes(xa, xb, n0 + HL, "S");

    // 12. Romantik nurlar (Romantic Lighting)
    // Markaziy iliq pushti-shafaq nuri
    const marryMeMainLight = new THREE.PointLight(0xff2a6d, 4.0, 22, 1.1);
    marryMeMainLight.position.set(cx, 3.9, -c);
    scene.add(marryMeMainLight);

    // Markaziy asar va arka uchun fokuslangan iliq oltin nur
    const centerSpot = new THREE.SpotLight(0xfff0dd, 5.5, 18, Math.PI / 4, 0.4, 1.1);
    centerSpot.position.set(cx + (farX - cx) * 0.4, 4.8, -c);
    centerSpot.target.position.set(outer, 2.5, -c);
    scene.add(centerSpot);
    scene.add(centerSpot.target);

    // Shamlar shulasi nuri
    const candleGlow = new THREE.PointLight(0xffa055, 3.2, 16, 1.3);
    candleGlow.position.set(heartX, 2.2, -c);
    scene.add(candleGlow);

  } else if (isLove) {
    // Qizil baxmal gilamlar (Red carpets)
    rug(cx - 2.8, cx + 2.8, c - 2.8, c + 2.8, "#80081d");
    // Eshikdan xona markaziga tomon qizil gilam yo'lak
    redRunner(inner, cx, c - 1.25, c + 1.25);
    // Markazdan asosiy ro'paradagi asarga tomon qizil gilam yo'lak
    redRunner(cx, outer, c - 1.25, c + 1.25);
    // Markazdan chap va o'ng asarlarga tomon qizil gilam yo'laklar
    redRunner(cx - 1.25, cx + 1.25, n0 + 0.6, c);
    redRunner(cx - 1.25, cx + 1.25, c, n0 + HL - 0.6);

    // Hashamatli qizil atirgullar (Roses)
    const doorX = inner + (side < 0 ? -1.0 : 1.0);
    roseVase(doorX, c - 2.2, 1.1);
    roseVase(doorX, c + 2.2, 1.1);

    const farX = outer - (side < 0 ? -1.2 : 1.2);
    roseVase(farX, c - 2.4, 1.15);
    roseVase(farX, c + 2.4, 1.15);

    const cornerX = inner + (side < 0 ? -2.2 : 2.2);
    roseVase(cornerX, n0 + 1.8, 1.0);
    roseVase(cornerX, n0 + HL - 1.8, 1.0);

    // Pol va gilamlar bo'ylab to'kilgan mayin atirgul barglari
    rosePetals(xa + 1.0, xb - 1.0, n0 + 1.0, n0 + HL - 1.0, 180);

    // Romantik iliq shafaq nuri (Romantic atmosphere)
    const romanticLight = new THREE.PointLight(0xff6b8b, 3.5, 18, 1.2);
    romanticLight.position.set(cx, 3.8, -c);
    scene.add(romanticLight);
    const warmCandleGlow = new THREE.PointLight(0xffb07c, 2.2, 14, 1.3);
    warmCandleGlow.position.set(farX, 2.7, -c);
    scene.add(warmCandleGlow);
  } else {
    rug(cx - 2.2, cx + 2.2, c - 2.4, c + 2.4, hall.accent);
  }

  wall(outer, n0, outer, n0 + HL, wm);
  wall(xa, n0, xb, n0, wm);
  wall(xa, n0 + HL, xb, n0 + HL, wm);

  const room = { x1: xa, x2: xb, n1: n0, n2: n0 + HL };
  const innerSide = side < 0 ? "E" : "W";
  trimRoom(room, { [innerSide]: { gaps: [{ c, w: DOOR }] } }, { panel: wallMat("wall_light", tones.panel) });

  if (isMarryMe) {
    bench(cx, n0 + 2.4, false, M.velvetRed);
    bench(cx, n0 + HL - 2.4, false, M.velvetRed);
  } else {
    bench(cx, c, true, isLove ? M.velvetRed : plainMat(tones.cushion, 1));
  }

  // nomlar: eshik ustida (koridor tomondan) va tashqi devorda
  const count = hall.paintings.length;
  const unit = sculpt ? "haykal" : ((isLove || isMarryMe) ? "romantik asar" : "asar");
  const slotsLabel = isMarryMe ? "1 ta markaziy asar" : (isLove ? "3 ta saralangan asar" : `${hall.slots} o'rin`);
  panelSign(inner, DOOR_H + 0.95, c, side < 0 ? "E" : "W", 3.8, 0.9,
    signTexture({ title: isMarryMe ? "Marry Me 💍" : hall.name, sub: `${count} ta ${unit} · ${slotsLabel}`, accent: hall.accent, titleSize: 74, subSize: 30 }), 0.12);
  panelSign(outer, 4.65, c, side < 0 ? "E" : "W", 6, 1.17,
    signTexture({ w: 1536, h: 300, style: "wall", title: isMarryMe ? "WILL YOU MARRY ME? 💍" : hall.name, sub: isMarryMe ? "Menga turmushga chiqasanmi? — Abadiy sevgi izhori" : hall.tagline, accent: hall.accent, titleSize: 120, subSize: 40 }));

  // asarlar: Marry Me zalida faqat 1 ta o'rin; Love Story zalida 3 ta; boshqa zallarda 8 ta o'rin
  const bySlot = new Map(hall.paintings.map((p) => [p.slot, p]));
  const spots = isMarryMe ? marryMeSlots(side, n0) : (isLove ? loveSlots(side, n0) : (sculpt ? sculptureSlots(side, n0) : paintingSlots(side, n0)));
  const maxSlots = isMarryMe ? 1 : (isLove ? 3 : hall.slots);
  spots.slice(0, maxSlots).forEach((s, i) => {
    const slot = i + 1, p = bySlot.get(slot);
    if (p) p.hallSlug = hall.slug;
    if (sculpt) p ? placeSculpture(p, hall, s.x, s.n, s.facing) : emptyPedestal(hall, slot, s.x, s.n, s.facing);
    else p ? hang(p, s.x, s.n, s.facing, hall.accent) : emptyFrame(hall, slot, s.x, s.n, s.facing);
  });

  rooms.push({
    kind: "hall", slug: hall.slug, label: isMarryMe ? "Marry Me 💍" : hall.name, accent: hall.accent,
    x1: xa, x2: xb, n1: n0, n2: n0 + HL, door: c, side,
    spawn: { x: inner + side * 2.2, n: c, yaw: side < 0 ? Math.PI / 2 : -Math.PI / 2 },
  });
}

// ---------------------------------------------------------------- binoni qurish
const halls = DATA.halls;
const pairs = [];
for (let i = 0; i < halls.length; i += 2) pairs.push([halls[i], halls[i + 1]]);

let nCursor = LL + 2;
const leftDoors = [], rightDoors = [], gaps = [];
for (const [Lh, Rh] of pairs) {
  buildHall(Lh, -1, nCursor);
  leftDoors.push({ c: nCursor + HL / 2, w: DOOR });
  if (Rh) {
    buildHall(Rh, 1, nCursor);
    rightDoors.push({ c: nCursor + HL / 2, w: DOOR });
  }
  nCursor += HL + GAP;
  gaps.push(nCursor - GAP / 2);
}
const END = Math.max(LL + 12, nCursor + 2);

// ---------------- Lobbi
const lobbyWall = wallMat("wall_warm_light", "#e4d8c3");
flat(-LW / 2, LW / 2, 0, LL, M.marble, 0, true);
flat(-LW / 2, LW / 2, 0, LL, M.ceiling, H, false, 4);
wall(-LW / 2, 0, LW / 2, 0, lobbyWall);
wall(-LW / 2, 0, -LW / 2, LL, lobbyWall);
wall(LW / 2, 0, LW / 2, LL, lobbyWall);
wall(-LW / 2, LL, LW / 2, LL, lobbyWall, [{ c: 0, w: CW, h: H - 0.9 }]);
trimRoom({ x1: -LW / 2, x2: LW / 2, n1: 0, n2: LL },
  { N: { gaps: [{ c: 0, w: CW }] }, S: { gaps: [{ c: 0, w: 3.8 }] } }, { panel: M.wood, rail: M.gold });

// pol medalyoni
{
  const [c, g] = makeCanvas(1024, 1024);
  g.fillStyle = "#e9e3d6"; g.fillRect(0, 0, 1024, 1024);
  const ring = (r, w, col) => { g.beginPath(); g.arc(512, 512, r, 0, Math.PI * 2); g.lineWidth = w; g.strokeStyle = col; g.stroke(); };
  g.beginPath(); g.arc(512, 512, 500, 0, Math.PI * 2); g.fillStyle = "#2a2f3a"; g.fill();
  g.beginPath(); g.arc(512, 512, 440, 0, Math.PI * 2); g.fillStyle = "#efe8da"; g.fill();
  ring(470, 14, "#c9a243"); ring(440, 6, "#c9a243");
  g.save(); g.translate(512, 512);
  for (let i = 0; i < 16; i++) {
    g.rotate(Math.PI / 8);
    g.beginPath(); g.moveTo(0, -420); g.lineTo(i % 2 ? 40 : 70, 0); g.lineTo(0, 0); g.closePath();
    g.fillStyle = i % 2 ? "#8c6d2c" : "#c9a243"; g.fill();
    g.beginPath(); g.moveTo(0, -420); g.lineTo(i % 2 ? -40 : -70, 0); g.lineTo(0, 0); g.closePath();
    g.fillStyle = i % 2 ? "#5a4720" : "#a8872f"; g.fill();
  }
  g.restore();
  g.beginPath(); g.arc(512, 512, 110, 0, Math.PI * 2); g.fillStyle = "#2a2f3a"; g.fill();
  ring(110, 10, "#c9a243");
  const m = new THREE.Mesh(new THREE.CircleGeometry(2.9, 64), std({ map: canvasTex(c), roughness: 0.22, metalness: 0.05 }));
  m.rotation.x = -Math.PI / 2;
  m.position.set(0, 0.006, -7);
  scene.add(m);
}

// markaziy haykal (sekin aylanadi)
const sculpture = new THREE.Mesh(new THREE.TorusKnotGeometry(0.42, 0.12, 200, 24, 2, 3), M.gold);
{
  const ped = new THREE.Mesh(new THREE.CylinderGeometry(0.62, 0.7, 1.05, 40), std({ map: tex("floor_marble"), color: 0x2c303b, roughness: 0.2 }));
  ped.position.set(0, 0.525, -7);
  scene.add(ped);
  for (const y of [0.05, 1.05]) {
    const r = new THREE.Mesh(new THREE.TorusGeometry(y < 1 ? 0.7 : 0.63, 0.03, 8, 40), M.gold);
    r.rotation.x = Math.PI / 2; r.position.set(0, y, -7); scene.add(r);
  }
  sculpture.position.set(0, 1.78, -7);
  scene.add(sculpture);
  addSolid(-0.9, 0.9, -7.9, -6.1);
}

// shiftdagi oyna (skylight)
{
  const [c, g] = makeCanvas(512, 512);
  const grad = g.createLinearGradient(0, 0, 0, 512);
  grad.addColorStop(0, "#f7fbff"); grad.addColorStop(1, "#dfe9f2");
  g.fillStyle = grad; g.fillRect(0, 0, 512, 512);
  g.strokeStyle = "#b08d3c"; g.lineWidth = 10;
  for (let i = 0; i <= 512; i += 128) { g.beginPath(); g.moveTo(i, 0); g.lineTo(i, 512); g.stroke(); g.beginPath(); g.moveTo(0, i); g.lineTo(512, i); g.stroke(); }
  g.lineWidth = 3; g.globalAlpha = 0.6;
  g.beginPath(); g.arc(256, 256, 180, 0, Math.PI * 2); g.stroke();
  const m = new THREE.Mesh(new THREE.PlaneGeometry(7, 7), new THREE.MeshBasicMaterial({ map: canvasTex(c) }));
  m.rotation.x = Math.PI / 2;
  m.position.set(0, H - 0.015, -7);
  scene.add(m);
  for (const [x, n, sx, sz] of [[0, 3.4, 7.6, 0.3], [0, 10.6, 7.6, 0.3], [-3.65, 7, 0.3, 7.6], [3.65, 7, 0.3, 7.6]]) {
    box(x, H - 0.12, n, sx, 0.24, sz, M.ivory);
  }
}

for (const [x, n] of [[-7.4, 3.2], [7.4, 3.2], [-7.4, 11.2], [7.4, 11.2]]) column(x, n);
for (const [x, n] of [[-4.6, 1.8], [4.6, 1.8], [-4.6, 12.2], [4.6, 12.2]]) downlight(x, n, 0.26);

// qabulxona stoli
box(-7.75, 0.55, 7, 0.9, 1.1, 4.0, M.wood, true);
box(-7.75, 1.12, 7, 1.1, 0.05, 4.2, M.ivory);
box(-7.28, 0.55, 7, 0.04, 0.9, 3.6, M.gold);
panelSign(-LW / 2, 2.4, 7, "E", 2.8, 0.7, signTexture({ title: "Ma'lumot", sub: "Qabulxona", titleSize: 80, subSize: 34 }));

// kirish eshiklari (ikki tavaqali)
box(0, 2.05, 0, 4.0, 4.1, T + 0.16, M.wood);
for (const s of [-1, 1]) {
  box(s * 0.92, 1.95, 0.24, 1.75, 3.8, 0.08, M.wood);
  box(s * 0.92, 2.6, 0.29, 1.3, 1.6, 0.02, plainMat("#3b2a1c", 0.6));
  box(s * 0.92, 1.0, 0.29, 1.3, 0.9, 0.02, plainMat("#3b2a1c", 0.6));
  box(s * 0.16, 1.9, 0.32, 0.05, 0.55, 0.05, M.gold);
}
box(0, 4.15, 0.2, 4.4, 0.12, 0.3, M.gold);
panelSign(0, 4.65, 0, "N", 3.2, 0.62, signTexture({ w: 1024, h: 200, title: "Chiqish", titleSize: 92 }), 0.1);

// lobbi yozuvlari
const visitorLine = DATA.visitor ? `Xush kelibsiz, ${DATA.visitor}` : "Xush kelibsiz";
panelSign(0, H - 0.45, LL, "S", 4.4, 0.62, signTexture({ w: 1024, h: 144, title: "Zallar", titleSize: 96 }), 0.1);
panelSign(-6, 2.75, LL, "S", 4.6, 1.5, signTexture({ w: 1024, h: 334, title: "Virtual Museum", sub: visitorLine, titleSize: 108, subSize: 34 }));
panelSign(6, 2.75, LL, "S", 4.6, 1.5, signTexture({
  w: 1024, h: 334, title: "Tarix · San'at · Bilim", sub: "WASD — yurish   ·   E — asar haqida", titleSize: 92, subSize: 30,
}));
panelSign(LW / 2, 2.75, 7, "W", 5.0, Math.max(2.2, 1.1 + halls.length * 0.42), signTexture({
  w: 1024, h: Math.max(450, 220 + halls.length * 84), title: "Zallar", titleSize: 92, lineSize: 46, align: "left",
  lines: halls.length
    ? halls.map((hl, i) => ({ text: `${i % 2 ? "→" : "←"}   ${hl.name}   (${hl.paintings.length})`, color: hl.accent }))
    : [{ text: "Hali zallar qo'shilmagan" }],
}));
plant(-2.6, 13.2); plant(2.6, 13.2);
bench(-4.2, 7, true, M.velvetRed);
bench(4.2, 7, true, M.velvetRed);
rooms.push({ kind: "lobby", label: "Lobbi", accent: "#d4af37", x1: -LW / 2, x2: LW / 2, n1: 0, n2: LL,
  spawn: { x: 0, n: 1.6, yaw: 0 } });

// ---------------- Koridor
const corrWall = wallMat("wall_warm_light", "#ddd0b9");
const doorCasing = M.wood;
flat(-CW / 2, CW / 2, LL, END, M.marble, 0, true);
runner(-1.25, 1.25, LL - 0.2, END - 0.6);
flat(-CW / 2, CW / 2, LL, END, M.ceiling, H, false, 4);
wall(-CW / 2, LL, -CW / 2, END, corrWall, leftDoors.map((d) => ({ ...d, casing: doorCasing })));
wall(CW / 2, LL, CW / 2, END, corrWall, rightDoors.map((d) => ({ ...d, casing: doorCasing })));
wall(-CW / 2, END, CW / 2, END, corrWall);
trimRoom({ x1: -CW / 2, x2: CW / 2, n1: LL, n2: END },
  { S: { skip: true }, W: { gaps: leftDoors }, E: { gaps: rightDoors } }, { panel: M.wood, rail: M.gold });
for (let n = LL + 2.5; n < END - 1; n += 4) downlight(0, n);
panelSign(0, 2.75, END, "S", 4.6, 1.2, signTexture({ title: "Muzey xaritasi — M", sub: "Esc — menyu va zallar ro'yxati", titleSize: 80, subSize: 30 }));
rooms.push({ kind: "corridor", label: "Koridor", accent: "#d4af37", x1: -CW / 2, x2: CW / 2, n1: LL, n2: END,
  spawn: { x: 0, n: LL + 1, yaw: 0 } });
for (const n of gaps) {
  if (n < END - 1.5) { plant(-2.45, n); plant(2.45, n); }
}

// ---------------------------------------------------------------- yo'naltirilgan chiroqlar (faqat yaqin asarlarga)
// Chiroqlar soni o'zgarmaydi (shader qayta kompilyatsiya qilinmaydi) — ular tashrifchiga
// eng yaqin asarlar ustiga silliq o'tkaziladi.
const SPOT_N = HIGH ? 8 : 0, SPOT_I = 22;
const spots = [];
for (let i = 0; i < SPOT_N; i++) {
  const s = new THREE.SpotLight(0xffeccd, 0, 8, 0.48, 0.6, 1.3);
  scene.add(s, s.target);
  spots.push({ light: s, item: null, level: 0 });
}
let spotTimer = 0;
function updateSpots(dt) {
  if (!SPOT_N) return;
  spotTimer -= dt;
  if (spotTimer <= 0) {
    spotTimer = 0.25;
    const p = player.position;
    const wanted = litSpots
      .map((it) => ({ it, d: (it.target.x - p.x) ** 2 + (it.target.z - p.z) ** 2 }))
      .filter((o) => o.d < 18 * 18)
      .sort((a, b) => a.d - b.d).slice(0, SPOT_N).map((o) => o.it);
    const covered = new Set(spots.filter((s) => wanted.includes(s.item)).map((s) => s.item));
    const free = wanted.filter((it) => !covered.has(it));
    for (const s of spots) {
      s.want = wanted.includes(s.item);
      if (!s.want && s.level < 0.02 && free.length) {
        s.item = free.shift();
        s.light.position.copy(s.item.pos);
        s.light.target.position.copy(s.item.target);
        s.light.target.updateMatrixWorld();
        s.want = true;
      }
    }
  }
  for (const s of spots) {
    s.level += ((s.want ? 1 : 0) - s.level) * Math.min(1, dt * 5);
    s.light.intensity = s.level * SPOT_I;
  }
}

// ---------------------------------------------------------------- harakat va to'qnashuv
function blocked(x, z) {
  for (const s of solids) {
    if (x + R > s.minX && x - R < s.maxX && z + R > s.minZ && z - R < s.maxZ) return true;
  }
  return false;
}
let pitch = 0;
function teleport(sp) {
  player.position.set(sp.x, EYE, -sp.n);
  player.rotation.y = sp.yaw;
  pitch = 0;
  camera.rotation.x = 0;
  spotTimer = 0;
}
const startRoom = rooms.find((r) => r.slug && r.slug === DATA.start);
teleport(startRoom ? startRoom.spawn : rooms.find((r) => r.kind === "lobby").spawn);

function roomAt(x, z) {
  const n = -z;
  return rooms.find((r) => x >= r.x1 && x <= r.x2 && n >= r.n1 && n <= r.n2);
}

// ---------------------------------------------------------------- UI elementlari
const $ = (id) => document.getElementById(id);
const menu = $("menu"), info = $("info"), promptEl = $("prompt"), whereEl = $("where");
const minimap = $("minimap");

// Mobil telefon/planshet (sensorli) vs Laptop/Kompyuter (mishka va klaviatura)
function checkIsMobile() {
  return /Android|webOS|iPhone|iPad|iPod|BlackBerry|IEMobile|Opera Mini/i.test(navigator.userAgent)
    || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1 && !matchMedia("(pointer: fine)").matches)
    || (matchMedia("(max-width: 860px) and (pointer: coarse)").matches && !matchMedia("(hover: hover)").matches);
}
let isMobile = checkIsMobile();

function applyDeviceClass() {
  isMobile = checkIsMobile();
  if (isMobile) {
    document.body.classList.add("is-mobile");
    document.body.classList.remove("is-desktop");
  } else {
    document.body.classList.add("is-desktop");
    document.body.classList.remove("is-mobile");
  }
}
applyDeviceClass();

// ---------------------------------------------------------------- nishonga olish
const ray = new THREE.Raycaster();
const center = new THREE.Vector2(0, 0);
let currentMouseNdc = center;

function pick(ndc, maxDist = 6) {
  ray.setFromCamera(ndc, camera);
  ray.far = maxDist;
  const hit = ray.intersectObjects(clickable, false)[0];
  return hit ? hit.object.userData.painting : null;
}

const keys = {};
let active = false, panelOpen = false, hovered = null;

// grafika sifati tugmasi
const qBtn = $("quality");
if (qBtn) {
  qBtn.textContent = HIGH ? "Grafika: Yuqori" : "Grafika: Tejamkor";
  qBtn.addEventListener("click", () => {
    try { localStorage.setItem("museumQuality", HIGH ? "low" : "high"); } catch { /* */ }
    location.reload();
  });
}

const hallList = $("hall-list");
for (const r of [...rooms.filter((q) => q.kind === "lobby"), ...rooms.filter((q) => q.kind === "hall")]) {
  const b = document.createElement("button");
  b.className = "hall-btn";
  b.style.setProperty("--accent", r.accent);
  const icon = document.createElement("span");
  icon.textContent = r.kind === "hall" ? "◆" : "•";
  b.append(icon, document.createTextNode(r.kind === "lobby" ? "Lobbi (kirish)" : r.label));
  if (r.kind === "hall") {
    const cnt = document.createElement("small");
    const hl = halls.find((h) => h.slug === r.slug);
    cnt.textContent = `${hl.paintings.length} / ${hl.slots} · ${hl.kind === "sculptures" ? "haykal" : "asar"}`;
    b.append(cnt);
  }
  b.addEventListener("click", () => { teleport(r.spawn); start(); });
  hallList.appendChild(b);
}

function start() {
  menu.hidden = true;
  active = true;
  applyDeviceClass();
  const pad = $("touchpad");
  if (isMobile) {
    if (pad) pad.classList.add("on");
  } else {
    if (pad) pad.classList.remove("on");
    try { canvas.requestPointerLock(); } catch {}
  }
}
$("enter").addEventListener("click", start);

function look(dx, dy, k) {
  player.rotation.y -= dx * k;
  pitch = Math.max(-1.2, Math.min(1.2, pitch - dy * k));
  camera.rotation.x = pitch;
}

// ---------------------------------------------------------------- Laptop: Mishka bilan boshqarish
let isMouseDown = false;
let lastMouseX = 0, lastMouseY = 0;
let mouseMovedDistance = 0;

canvas.addEventListener("mousedown", (e) => {
  if (e.button === 0 && !panelOpen) {
    isMouseDown = true;
    lastMouseX = e.clientX;
    lastMouseY = e.clientY;
    mouseMovedDistance = 0;
  }
});

document.addEventListener("mousemove", (e) => {
  if (!active || panelOpen) return;

  const isLocked = document.pointerLockElement === canvas;

  if (isLocked) {
    // 1. Pointer Lock rejimida sichqoncha harakati (FPS kamera)
    look(e.movementX, e.movementY, 0.0022);
    currentMouseNdc = center;
  } else if (isMouseDown) {
    // 2. Pointer lock yo'qligida sichqonchani bosib ushlagan holda aylantirish (Mouse Drag)
    const dx = e.clientX - lastMouseX;
    const dy = e.clientY - lastMouseY;
    lastMouseX = e.clientX;
    lastMouseY = e.clientY;
    mouseMovedDistance += Math.hypot(dx, dy);
    look(dx, dy, 0.0035);
  } else if (!isMobile) {
    // 3. Pointer lock yo'qligida kursorni asarlar ustiga borganda ko'rsatish
    const rect = canvas.getBoundingClientRect();
    currentMouseNdc = new THREE.Vector2(
      ((e.clientX - rect.left) / rect.width) * 2 - 1,
      -((e.clientY - rect.top) / rect.height) * 2 + 1
    );
  }
});

document.addEventListener("mouseup", (e) => {
  if (e.button === 0) {
    isMouseDown = false;
  }
});

canvas.addEventListener("click", (e) => {
  if (panelOpen) return;

  // Agar sichqoncha bilan burilish uchun surilgan bo'lsa (drag), click deb hisoblamaymiz
  if (mouseMovedDistance > 6) {
    mouseMovedDistance = 0;
    return;
  }

  // Pointer lock yo'qligida to'g'ridan-to'g'ri sichqoncha bosilgan nuqtadagi asarni ochish
  if (document.pointerLockElement !== canvas) {
    const rect = canvas.getBoundingClientRect();
    const mouseNdc = new THREE.Vector2(
      ((e.clientX - rect.left) / rect.width) * 2 - 1,
      -((e.clientY - rect.top) / rect.height) * 2 + 1
    );
    const hit = pick(mouseNdc, 8);
    if (hit) return openInfo(hit);
  }

  // Pointer lock rejimida markazda qaratilgan asarni ochish
  if (active && hovered) return openInfo(hovered);

  // Laptopda agar pointer lock hali bo'lmasa, canvasga bosganda qulflash
  if (!isMobile) {
    try { canvas.requestPointerLock(); } catch {}
  }
});

document.addEventListener("pointerlockchange", () => {
  const isLocked = document.pointerLockElement === canvas;
  if (isLocked) {
    active = true;
    menu.hidden = true;
  } else if (!panelOpen && !isMobile) {
    // Laptopda pointer lockdan chiqqanda (masalan Esc bosilganda) menyu ochiladi
    active = false;
    menu.hidden = false;
    const enterBtn = $("enter");
    if (enterBtn) enterBtn.textContent = "Davom etish";
  }
});

document.addEventListener("pointerlockerror", () => {
  active = true;
  menu.hidden = true;
});

// ---------------------------------------------------------------- Telefon: Sensorli ekran boshqaruvi
let lastT = null, tapStart = null;
canvas.addEventListener("touchstart", (e) => {
  if (isMobile) {
    const pad = $("touchpad");
    if (pad && !pad.classList.contains("on")) {
      pad.classList.add("on");
    }
  }
  const t = e.touches[0];
  lastT = { x: t.clientX, y: t.clientY };
  tapStart = { ...lastT, at: performance.now() };
}, { passive: true });
canvas.addEventListener("touchmove", (e) => {
  if (!active || panelOpen || !lastT) return;
  const t = e.touches[0];
  look(t.clientX - lastT.x, t.clientY - lastT.y, 0.005);
  lastT = { x: t.clientX, y: t.clientY };
}, { passive: true });
canvas.addEventListener("touchend", (e) => {
  const t = e.changedTouches[0];
  if (tapStart && Math.hypot(t.clientX - tapStart.x, t.clientY - tapStart.y) < 10 && performance.now() - tapStart.at < 300) {
    const rect = canvas.getBoundingClientRect();
    const hit = pick(new THREE.Vector2(((t.clientX - rect.left) / rect.width) * 2 - 1,
      -((t.clientY - rect.top) / rect.height) * 2 + 1), 9);
    if (hit) openInfo(hit);
  }
  lastT = tapStart = null;
});
document.querySelectorAll("#touchpad [data-key]").forEach((b) => {
  const k = b.dataset.key;
  b.addEventListener("touchstart", (e) => { e.preventDefault(); keys[k] = true; });
  b.addEventListener("touchend", () => { keys[k] = false; });
  b.addEventListener("touchcancel", () => { keys[k] = false; });
});
$("touch-menu").addEventListener("click", () => { active = false; menu.hidden = false; $("enter").textContent = "Davom etish"; });

// ---------------------------------------------------------------- Laptop: Klaviatura boshqaruvi
addEventListener("keydown", (e) => {
  if (e.target && (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA")) return;
  keys[e.code] = true;
  if (e.key) keys[e.key.toLowerCase()] = true;

  // E yoki e (kirill 'у') bosilganda asar haqida ma'lumot
  if ((e.code === "KeyE" || e.key === "e" || e.key === "E" || e.key === "у" || e.key === "У") && hovered && active && !panelOpen) {
    openInfo(hovered);
  }
  // M mini-xarita
  if (e.code === "KeyM" || e.key === "m" || e.key === "M" || e.key === "ь" || e.key === "Ь") {
    minimap.classList.toggle("off");
  }
  // Escape panellarni yopish yoki menyuga chiqish
  if (e.code === "Escape") {
    if (panelOpen) {
      closeInfo();
    } else if (active) {
      if (document.pointerLockElement) document.exitPointerLock();
      active = false;
      menu.hidden = false;
      const enterBtn = $("enter");
      if (enterBtn) enterBtn.textContent = "Davom etish";
    }
  }
  // Menyu ochiq turganda Enter yoki Bo'shliq (Space) orqali kirish
  if (!active && !menu.hidden && (e.code === "Enter" || e.code === "Space")) {
    start();
  }
});
addEventListener("keyup", (e) => {
  keys[e.code] = false;
  if (e.key) keys[e.key.toLowerCase()] = false;
});
addEventListener("blur", () => {
  for (const k in keys) keys[k] = false;
  isMouseDown = false;
});

// ---------------------------------------------------------------- ma'lumot paneli
function openInfo(p) {
  panelOpen = true;
  promptEl.hidden = true;
  for (const k in keys) keys[k] = false;
  const isReadOnly = Boolean(DATA.read_only);
  const replace = $("info-replace");
  if (isReadOnly || !p.replace) {
    replace.hidden = true;
    replace.style.display = "none";
  } else {
    replace.hidden = false;
    replace.style.display = "";
    replace.href = p.replace;
  }
  if (p.empty) {   // bo'sh o'rin
    $("info-img").hidden = true;
    $("info-title").textContent = `${p.slot}-o'rin bo'sh`;
    $("info-artist").textContent = p.hallName;
    $("info-desc").textContent = isReadOnly
      ? "Bu o'rinda hozircha hech qanday asar joylashtirilmagan."
      : (p.sculpt
        ? "Bu postamentga o'z haykalingizni qo'ying: rasm yuklang — dastur uni avtomatik ravishda marmar haykalga aylantirib, shu yerga qo'yadi."
        : "Bu o'ringa o'z rasmingizni qo'yishingiz mumkin. Yuklagan rasmingizni faqat siz va administrator ko'radi.");
    $("info-fact").textContent = "";
    $("info-fact-h").hidden = true;
    $("info-owner").textContent = "";
    $("info-link").hidden = true;
    $("info-link").style.display = "none";
    if (!isReadOnly) {
      replace.textContent = p.sculpt ? "Haykal qo'yish" : "Rasm qo'yish";
    }
  } else {
    const sculpt = !!p.depth;
    $("info-img").hidden = false;
    $("info-img").src = p.image;
    $("info-title").textContent = p.title;
    $("info-artist").textContent = p.artist + (p.year ? " · " + p.year : "");
    $("info-desc").textContent = p.description || "";
    $("info-fact").textContent = p.facts || "";
    $("info-fact-h").hidden = !p.facts;
    $("info-owner").textContent = (p.mine ? "Bu sizning asaringiz" : (p.owner ? "Yuklagan: @" + p.owner : "Muzey fondidan"))
      + ` · ${p.slot}-o'rin` + (sculpt ? " · rasmdan yasalgan haykal" : "");
    if (isReadOnly || !p.url) {
      $("info-link").hidden = true;
      $("info-link").style.display = "none";
    } else {
      $("info-link").hidden = false;
      $("info-link").style.display = "";
      $("info-link").href = p.url;
    }
    if (!isReadOnly) {
      replace.textContent = p.mine ? (sculpt ? "Haykalni almashtirish" : "Rasmni almashtirish")
        : (p.hallSlug && halls.find((h) => h.slug === p.hallSlug)?.kind === "sculptures" ? "O'z haykalim bilan almashtirish" : "O'z rasmim bilan almashtirish");
    }
  }
  info.hidden = false;
  if (document.pointerLockElement) document.exitPointerLock();
}
function closeInfo() {
  info.hidden = true;
  panelOpen = false;
  if (!isMobile) {
    try { canvas.requestPointerLock(); } catch {}
  }
}
$("info-close").addEventListener("click", closeInfo);
info.addEventListener("click", (e) => { if (e.target === info) closeInfo(); });

// ---------------------------------------------------------------- mini-xarita
const mm = (() => {
  const minX = -(CW / 2 + D) - 1, maxX = CW / 2 + D + 1, minN = -1, maxN = END + 1;
  const scale = Math.min(170 / (maxX - minX), 230 / (maxN - minN));
  const w = Math.ceil((maxX - minX) * scale), h = Math.ceil((maxN - minN) * scale);
  minimap.width = w * 2; minimap.height = h * 2;
  minimap.style.width = w + "px"; minimap.style.height = h + "px";
  const g = minimap.getContext("2d");
  g.scale(2, 2);
  const X = (x) => (x - minX) * scale, Y = (n) => (maxN - n) * scale;
  const bg = document.createElement("canvas");
  bg.width = w * 2; bg.height = h * 2;
  const b = bg.getContext("2d");
  b.scale(2, 2);
  for (const r of rooms) {
    b.fillStyle = r.kind === "hall" ? r.accent + "55" : "rgba(236,231,220,0.12)";
    b.strokeStyle = r.kind === "hall" ? r.accent : "rgba(236,231,220,0.45)";
    b.lineWidth = 1;
    b.fillRect(X(r.x1), Y(r.n2), (r.x2 - r.x1) * scale, (r.n2 - r.n1) * scale);
    b.strokeRect(X(r.x1) + 0.5, Y(r.n2) + 0.5, (r.x2 - r.x1) * scale - 1, (r.n2 - r.n1) * scale - 1);
  }
  minimap.addEventListener("click", (e) => {
    const rect = minimap.getBoundingClientRect();
    const x = (e.clientX - rect.left) / scale + minX, n = maxN - (e.clientY - rect.top) / scale;
    const r = rooms.find((q) => x >= q.x1 && x <= q.x2 && n >= q.n1 && n <= q.n2);
    if (r) { teleport(r.spawn); if (!active) start(); }
  });
  return () => {
    g.clearRect(0, 0, w, h);
    g.drawImage(bg, 0, 0, w, h);
    const px = X(player.position.x), py = Y(-player.position.z);
    const yaw = player.rotation.y;
    const dx = -Math.sin(yaw), dn = Math.cos(yaw);
    g.fillStyle = "rgba(212,175,55,0.28)";
    g.beginPath();
    g.moveTo(px, py);
    g.arc(px, py, 16, Math.atan2(-dn, dx) - 0.55, Math.atan2(-dn, dx) + 0.55);
    g.closePath();
    g.fill();
    g.fillStyle = "#d4af37";
    g.beginPath(); g.arc(px, py, 3.5, 0, Math.PI * 2); g.fill();
  };
})();

// ---------------------------------------------------------------- sikl
function resize() {
  const w = stage.clientWidth, h = stage.clientHeight;
  renderer.setSize(w, h, false);
  camera.aspect = w / h;
  camera.updateProjectionMatrix();
  applyDeviceClass();
  const pad = $("touchpad");
  if (pad) {
    if (isMobile && active && !panelOpen) pad.classList.add("on");
    else pad.classList.remove("on");
  }
}
addEventListener("resize", resize);
addEventListener("orientationchange", () => setTimeout(resize, 150));
resize();

const clock = new THREE.Clock();
const dir = new THREE.Vector3();
const UP = new THREE.Vector3(0, 1, 0);
let bob = 0, lastRoom = null;

function tick() {
  const dt = Math.min(clock.getDelta(), 0.25);   // sekin qurilmalarda ham to'g'ri tezlik
  if (active && !panelOpen) {
    dir.set(0, 0, 0);
    // Oldinga (W, ArrowUp, 'w', 'ц')
    if (keys.KeyW || keys.ArrowUp || keys.w || keys.ц) dir.z -= 1;
    // Orqaga (S, ArrowDown, 's', 'ы')
    if (keys.KeyS || keys.ArrowDown || keys.s || keys.ы) dir.z += 1;
    // Chapga strafe (A, 'a', 'ф')
    if (keys.KeyA || keys.a || keys.ф) dir.x -= 1;
    // O'ngga strafe (D, 'd', 'в')
    if (keys.KeyD || keys.d || keys.в) dir.x += 1;
    // Chapga burilish (ArrowLeft, TurnLeft, 'q', 'й')
    if (keys.ArrowLeft || keys.TurnLeft || keys.q || keys.й) player.rotation.y += 1.8 * dt;
    // O'ngga burilish (ArrowRight, TurnRight)
    if (keys.ArrowRight || keys.TurnRight) player.rotation.y -= 1.8 * dt;

    if (dir.lengthSq() > 0) {
      const speed = (keys.ShiftLeft || keys.ShiftRight || keys.shift) ? 6.5 : 3.4;
      dir.normalize().multiplyScalar(speed * dt).applyAxisAngle(UP, player.rotation.y);
      // devordan "sakrab o'tib ketmaslik" uchun harakatni mayda qadamlarga bo'lamiz
      const steps = Math.max(1, Math.ceil(dir.length() / 0.2));
      const sx = dir.x / steps, sz = dir.z / steps;
      for (let i = 0; i < steps; i++) {
        const { x, z } = player.position;
        if (!blocked(x + sx, z)) player.position.x += sx;
        if (!blocked(player.position.x, z + sz)) player.position.z += sz;
      }
      bob += dt * speed * 2.4;
      player.position.y = EYE + Math.sin(bob) * 0.03;
    }

    if (isMobile) {
      hovered = pick(center);
    } else if (document.pointerLockElement === canvas) {
      hovered = pick(center);
      canvas.style.cursor = "default";
    } else {
      hovered = pick(currentMouseNdc, 8);
      if (hovered) {
        canvas.style.cursor = "pointer";
      } else {
        canvas.style.cursor = isMouseDown ? "grabbing" : "grab";
      }
    }
    promptEl.hidden = !hovered;
  } else {
    hovered = null;
    promptEl.hidden = true;
  }
  sculpture.rotation.y += dt * 0.35;
  updateSpots(dt);
  const room = roomAt(player.position.x, player.position.z);
  if (room && room !== lastRoom) {
    lastRoom = room;
    whereEl.textContent = room.label;
    whereEl.style.setProperty("--accent", room.accent);
  }
  mm();
  renderer.render(scene, camera);
  requestAnimationFrame(tick);
}

// ---------------------------------------------------------------- statik geometriyani birlashtirish
// Bir xil materialdagi yuzlab mayda bo'laklar (plintus, ramka, ustun...) bitta mesh'ga
// yig'iladi: ko'rinish o'zgarmaydi, draw call soni ~10 barobar kamayadi.
function mergeStatic() {
  scene.updateMatrixWorld(true);
  const buckets = new Map();
  scene.traverse((o) => {
    if (!o.isMesh || o === sculpture || clickable.includes(o)) return;
    let list = buckets.get(o.material);
    if (!list) buckets.set(o.material, (list = []));
    list.push(o);
  });
  for (const [mat, list] of buckets) {
    if (list.length < 2) continue;
    const geos = list.map((o) => {
      const g = o.geometry.index ? o.geometry.toNonIndexed() : o.geometry.clone();
      g.applyMatrix4(o.matrixWorld);
      return g;
    });
    const names = ["position", "normal", "uv"].filter((nm) => geos.every((g) => g.attributes[nm]));
    const merged = new THREE.BufferGeometry();
    for (const nm of names) {
      const size = geos[0].attributes[nm].itemSize;
      const arr = new Float32Array(geos.reduce((acc, g) => acc + g.attributes[nm].array.length, 0));
      let off = 0;
      for (const g of geos) { arr.set(g.attributes[nm].array, off); off += g.attributes[nm].array.length; }
      merged.setAttribute(nm, new THREE.BufferAttribute(arr, size));
    }
    geos.forEach((g) => g.dispose());
    const m = new THREE.Mesh(merged, mat);
    m.frustumCulled = false;
    scene.add(m);
    for (const o of list) o.parent.remove(o);
  }
}
mergeStatic();

THREE.DefaultLoadingManager.onLoad = () => {
  $("loadstate").textContent = "Tayyor.";
  stage.classList.add("ready");
};
window.__museumReady = true;
$("loadstate").textContent = "Rasmlar yuklanmoqda…";
window.__museum = { rooms, solids, teleport, blocked, player, END, renderer, scene };   // tekshiruv uchun
tick();
