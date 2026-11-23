(() => {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;

  const canvas = document.getElementById('canvas');
  const ctx = canvas.getContext('2d');
  const toolbar = document.getElementById('toolbar');
  const dimBadge = document.getElementById('dimBadge');
  const hint = document.getElementById('hint');
  const beautifyBtn = document.getElementById('beautifyBtn');

  let bgImg = null;
  let dpr = 1;
  let mode = 'capture'; // 'capture' | 'ocr'
  let phase = 'idle';   // 'idle' | 'selecting' | 'editing'
  let sel = null;       // { x, y, w, h } in CSS px
  let tool = 'arrow';
  let color = '#ff375f';
  let beautify = false;
  let shapes = [];
  let drawing = null;

  function normRect(x0, y0, x1, y1) {
    return {
      x: Math.min(x0, x1),
      y: Math.min(y0, y1),
      w: Math.abs(x1 - x0),
      h: Math.abs(y1 - y0),
    };
  }

  function drawArrow(c, x1, y1, x2, y2, col, lw) {
    const head = 12;
    const angle = Math.atan2(y2 - y1, x2 - x1);
    c.strokeStyle = col;
    c.fillStyle = col;
    c.lineWidth = lw;
    c.lineCap = 'round';
    c.beginPath();
    c.moveTo(x1, y1);
    c.lineTo(x2, y2);
    c.stroke();
    c.beginPath();
    c.moveTo(x2, y2);
    c.lineTo(x2 - head * Math.cos(angle - Math.PI / 6), y2 - head * Math.sin(angle - Math.PI / 6));
    c.lineTo(x2 - head * Math.cos(angle + Math.PI / 6), y2 - head * Math.sin(angle + Math.PI / 6));
    c.closePath();
    c.fill();
  }

  function pixelateRect(c, sx, sy, sw, sh, scale) {
    if (sw < 4 || sh < 4 || !bgImg) return;
    const tmp = document.createElement('canvas');
    const block = 10;
    const tw = Math.max(1, Math.round(sw / block));
    const th = Math.max(1, Math.round(sh / block));
    tmp.width = tw;
    tmp.height = th;
    const tctx = tmp.getContext('2d');
    tctx.drawImage(bgImg, sx * scale, sy * scale, sw * scale, sh * scale, 0, 0, tw, th);
    c.save();
    c.imageSmoothingEnabled = false;
    c.drawImage(tmp, 0, 0, tw, th, sx, sy, sw, sh);
    c.restore();
  }

  function drawShape(c, s, ox, oy, scale) {
    if (s.type === 'arrow') {
      drawArrow(c, s.x1 - ox, s.y1 - oy, s.x2 - ox, s.y2 - oy, s.color, 3);
    } else if (s.type === 'rect') {
      const r = normRect(s.x1 - ox, s.y1 - oy, s.x2 - ox, s.y2 - oy);
      c.strokeStyle = s.color;
      c.lineWidth = 3;
      c.strokeRect(r.x, r.y, r.w, r.h);
    } else if (s.type === 'pen' && s.pts.length > 1) {
      c.strokeStyle = s.color;
      c.lineWidth = 3;
      c.lineCap = 'round';
      c.lineJoin = 'round';
      c.beginPath();
      c.moveTo(s.pts[0][0] - ox, s.pts[0][1] - oy);
      for (let i = 1; i < s.pts.length; i++) c.lineTo(s.pts[i][0] - ox, s.pts[i][1] - oy);
      c.stroke();
    } else if (s.type === 'blur') {
      const r = normRect(s.x1, s.y1, s.x2, s.y2);
      if (ox === 0 && oy === 0) {
        pixelateRect(c, r.x, r.y, r.w, r.h, scale);
      } else {
        const tmp = document.createElement('canvas');
        const block = 10;
        const tw = Math.max(1, Math.round(r.w / block));
        const th = Math.max(1, Math.round(r.h / block));
        tmp.width = tw;
        tmp.height = th;
        tmp.getContext('2d').drawImage(bgImg, r.x * scale, r.y * scale, r.w * scale, r.h * scale, 0, 0, tw, th);
        c.save();
        c.imageSmoothingEnabled = false;
        c.drawImage(tmp, 0, 0, tw, th, r.x - ox, r.y - oy, r.w, r.h);
        c.restore();
      }
    }
  }

  function redraw() {
    if (!bgImg) return;
    const cw = canvas.width / dpr;
    const ch = canvas.height / dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.drawImage(bgImg, 0, 0, cw, ch);

    ctx.fillStyle = 'rgba(0, 0, 0, 0.45)';
    ctx.fillRect(0, 0, cw, ch);

    if (sel && sel.w > 2 && sel.h > 2) {
      if (beautify) {
        const pad = 26;
        const bx = sel.x - pad;
        const by = sel.y - pad;
        const bw = sel.w + pad * 2;
        const bh = sel.h + pad * 2;
        const grad = ctx.createLinearGradient(bx, by, bx + bw, by + bh);
        grad.addColorStop(0, '#667eea');
        grad.addColorStop(0.5, '#764ba2');
        grad.addColorStop(1, '#f093fb');
        ctx.save();
        ctx.beginPath();
        ctx.roundRect(bx, by, bw, bh, 16);
        ctx.fillStyle = grad;
        ctx.fill();
        ctx.strokeStyle = 'rgba(191, 90, 242, 0.85)';
        ctx.lineWidth = 2;
        ctx.stroke();
        ctx.restore();

        ctx.save();
        ctx.shadowColor = 'rgba(0, 0, 0, 0.48)';
        ctx.shadowBlur = 28;
        ctx.shadowOffsetY = 10;
        ctx.beginPath();
        ctx.roundRect(sel.x, sel.y, sel.w, sel.h, 12);
        ctx.fillStyle = '#000';
        ctx.fill();
        ctx.restore();

        ctx.save();
        ctx.beginPath();
        ctx.roundRect(sel.x, sel.y, sel.w, sel.h, 12);
        ctx.clip();
        ctx.drawImage(bgImg, 0, 0, cw, ch);
        for (const s of shapes) drawShape(ctx, s, 0, 0, dpr);
        if (drawing) drawShape(ctx, drawing, 0, 0, dpr);
        ctx.restore();
      } else {
        ctx.save();
        ctx.beginPath();
        ctx.rect(sel.x, sel.y, sel.w, sel.h);
        ctx.clip();
        ctx.drawImage(bgImg, 0, 0, cw, ch);
        for (const s of shapes) drawShape(ctx, s, 0, 0, dpr);
        if (drawing) drawShape(ctx, drawing, 0, 0, dpr);
        ctx.restore();

        ctx.strokeStyle = '#7c5cff';
        ctx.lineWidth = 2;
        ctx.strokeRect(sel.x, sel.y, sel.w, sel.h);
      }

      dimBadge.hidden = false;
      dimBadge.textContent = `${Math.round(sel.w * dpr)} × ${Math.round(sel.h * dpr)}`;
      dimBadge.style.left = Math.max(8, beautify ? sel.x - 26 : sel.x) + 'px';
      dimBadge.style.top = Math.max(8, (beautify ? sel.y - 26 : sel.y) - 28) + 'px';
    } else {
      dimBadge.hidden = true;
    }
  }

  function placeToolbar() {
    if (!sel || sel.w < 4 || sel.h < 4) { toolbar.hidden = true; return; }
    toolbar.hidden = false;
    const tw = toolbar.offsetWidth || 460;
    const left = Math.min(window.innerWidth - tw - 12, Math.max(12, sel.x + sel.w - tw));
    const pad = beautify ? 26 : 0;
    const below = sel.y + sel.h + pad + 12;
    const top = below + 46 < window.innerHeight ? below : Math.max(12, sel.y - pad - 48);
    toolbar.style.left = left + 'px';
    toolbar.style.top = top + 'px';
  }

  function exportPngDataUrl() {
    if (!sel || !bgImg) return null;
    const pw = Math.max(1, Math.round(sel.w * dpr));
    const ph = Math.max(1, Math.round(sel.h * dpr));
    const raw = document.createElement('canvas');
    raw.width = pw;
    raw.height = ph;
    const rctx = raw.getContext('2d');
    rctx.drawImage(bgImg, sel.x * dpr, sel.y * dpr, pw, ph, 0, 0, pw, ph);
    rctx.save();
    rctx.scale(dpr, dpr);
    for (const s of shapes) drawShape(rctx, s, sel.x, sel.y, dpr);
    rctx.restore();

    if (!beautify) return raw.toDataURL('image/png');

    const pad = Math.round(36 * dpr);
    const rad = Math.round(14 * dpr);
    const out = document.createElement('canvas');
    out.width = pw + pad * 2;
    out.height = ph + pad * 2;
    const octx = out.getContext('2d');
    const grad = octx.createLinearGradient(0, 0, out.width, out.height);
    grad.addColorStop(0, '#667eea');
    grad.addColorStop(0.5, '#764ba2');
    grad.addColorStop(1, '#f093fb');
    octx.fillStyle = grad;
    octx.fillRect(0, 0, out.width, out.height);

    octx.save();
    octx.shadowColor = 'rgba(0, 0, 0, 0.45)';
    octx.shadowBlur = Math.round(32 * dpr);
    octx.shadowOffsetY = Math.round(12 * dpr);
    octx.beginPath();
    octx.roundRect(pad, pad, pw, ph, rad);
    octx.fillStyle = '#000';
    octx.fill();
    octx.restore();

    octx.save();
    octx.beginPath();
    octx.roundRect(pad, pad, pw, ph, rad);
    octx.clip();
    octx.drawImage(raw, pad, pad);
    octx.restore();
    return out.toDataURL('image/png');
  }

  async function doCopy() {
    const url = exportPngDataUrl();
    if (url) await invoke('copy_image', { dataUrl: url }).catch(() => {});
  }
  async function doSave() {
    const url = exportPngDataUrl();
    if (url) await invoke('save_image', { dataUrl: url }).catch(() => {});
  }
  async function doOcr() {
    const prev = beautify;
    beautify = false;
    const url = exportPngDataUrl();
    beautify = prev;
    if (url) await invoke('ocr_image', { dataUrl: url }).catch(() => {});
  }

  let sx = 0, sy = 0;
  canvas.addEventListener('pointerdown', (e) => {
    if (e.button !== 0 || !bgImg) return;
    if (phase === 'idle') {
      phase = 'selecting';
      sx = e.clientX;
      sy = e.clientY;
      sel = { x: sx, y: sy, w: 0, h: 0 };
      toolbar.hidden = true;
      hint.hidden = true;
    } else if (phase === 'editing' && sel) {
      const inside = e.clientX >= sel.x && e.clientX <= sel.x + sel.w && e.clientY >= sel.y && e.clientY <= sel.y + sel.h;
      if (!inside) {
        phase = 'selecting';
        shapes = [];
        sx = e.clientX;
        sy = e.clientY;
        sel = { x: sx, y: sy, w: 0, h: 0 };
        toolbar.hidden = true;
        return;
      }
      if (tool === 'pen') drawing = { type: 'pen', color, pts: [[e.clientX, e.clientY]] };
      else drawing = { type: tool, color, x1: e.clientX, y1: e.clientY, x2: e.clientX, y2: e.clientY };
    }
    canvas.setPointerCapture(e.pointerId);
    redraw();
  });

  canvas.addEventListener('pointermove', (e) => {
    if (phase === 'selecting') {
      sel = normRect(sx, sy, e.clientX, e.clientY);
      redraw();
    } else if (drawing) {
      if (drawing.type === 'pen') drawing.pts.push([e.clientX, e.clientY]);
      else { drawing.x2 = e.clientX; drawing.y2 = e.clientY; }
      redraw();
    }
  });

  canvas.addEventListener('pointerup', () => {
    if (phase === 'selecting') {
      if (!sel || sel.w < 6 || sel.h < 6) {
        phase = 'idle';
        sel = null;
        hint.hidden = false;
        redraw();
        return;
      }
      phase = 'editing';
      redraw();
      if (mode === 'ocr') {
        doOcr();
      } else {
        placeToolbar();
      }
    } else if (drawing) {
      shapes.push(drawing);
      drawing = null;
      redraw();
    }
  });

  toolbar.addEventListener('click', (e) => {
    const t = e.target.closest('[data-tool]');
    if (t) {
      tool = t.dataset.tool;
      toolbar.querySelectorAll('[data-tool]').forEach((b) => b.classList.toggle('active', b === t));
      return;
    }
    const c = e.target.closest('[data-color]');
    if (c) {
      color = c.dataset.color;
      toolbar.querySelectorAll('[data-color]').forEach((b) => b.classList.toggle('active', b === c));
      return;
    }
    const a = e.target.closest('[data-a]')?.dataset.a;
    if (a === 'beautify') {
      beautify = !beautify;
      beautifyBtn.classList.toggle('active', beautify);
      redraw();
      placeToolbar();
    } else if (a === 'undo') {
      shapes.pop();
      redraw();
    } else if (a === 'copy') doCopy();
    else if (a === 'save') doSave();
    else if (a === 'ocr') doOcr();
    else if (a === 'cancel') invoke('cancel_capture').catch(() => {});
  });

  window.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') invoke('cancel_capture').catch(() => {});
    else if (e.key === 'Enter' && phase === 'editing') doCopy();
    else if (e.ctrlKey && e.key.toLowerCase() === 'c' && phase === 'editing') doCopy();
    else if (e.ctrlKey && e.key.toLowerCase() === 's' && phase === 'editing') { e.preventDefault(); doSave(); }
    else if (e.ctrlKey && e.key.toLowerCase() === 'z' && phase === 'editing') { shapes.pop(); redraw(); }
  });

  listen('snapshot-ready', (e) => {
    const payload = e.payload || {};
    mode = payload.mode || 'capture';
    const screen = payload.screen;
    if (!screen || !screen.image) return;
    phase = 'idle';
    sel = null;
    shapes = [];
    drawing = null;
    toolbar.hidden = true;
    dimBadge.hidden = true;
    hint.hidden = false;
    hint.textContent = mode === 'ocr'
      ? 'Выделите область с текстом для OCR · Esc — отмена'
      : 'Выделите область мышью · Esc — отмена';

    const img = new Image();
    img.onload = () => {
      bgImg = img;
      dpr = window.devicePixelRatio || 1;
      canvas.width = Math.round(window.innerWidth * dpr);
      canvas.height = Math.round(window.innerHeight * dpr);
      redraw();
    };
    img.src = screen.image;
  });
})();
