'use strict';

const PAGE_SIZE = 20;
const form = document.getElementById('search-form');
const resultsEl = document.getElementById('results');
const prevBtn = document.getElementById('previous-page');
const nextBtn = document.getElementById('next-page');
const pageInfo = document.getElementById('page-info');

let currentParams = null; // 保留翻页用的搜索条件
let currentPage = 1;
let detailCache = new Map(); // id -> { lyrics, filename }

function escapeHtml(s) {
  return String(s == null ? '' : s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

// 转义 snippet，但保留后端生成的 <mark> 高亮标签
function escapeHtmlKeepMark(s) {
  return escapeHtml(s)
    .replace(/&lt;mark&gt;/g, '<mark>')
    .replace(/&lt;\/mark&gt;/g, '</mark>');
}

function readFormParams() {
  const fd = new FormData(form);
  const p = {};
  for (const k of ['q', 'musicName', 'artistName', 'albumName', 'lyricText']) {
    const v = (fd.get(k) || '').toString().trim();
    if (v) p[k] = v;
  }
  p.source = (fd.get('source') || 'all').toString();
  return p;
}

function showError(msg) {
  resultsEl.innerHTML = '<p class="error">' + escapeHtml(msg) + '</p>';
  prevBtn.disabled = true;
  nextBtn.disabled = true;
  pageInfo.textContent = '';
}

async function doSearch(page) {
  const params = new URLSearchParams(currentParams);
  params.set('page', String(page));
  params.set('pageSize', String(PAGE_SIZE));
  resultsEl.innerHTML = '<p class="hint">搜索中…</p>';
  let json;
  try {
    const resp = await fetch('/api/v1/lyrics/search?' + params.toString());
    json = await resp.json();
  } catch (e) {
    showError('请求失败：' + e.message);
    return;
  }
  if (json.status !== 200) {
    showError(json.message || ('搜索失败（status=' + json.status + '）'));
    return;
  }
  currentPage = page;
  renderResults(json.data);
}

function renderResults(data) {
  const items = data.items || [];
  if (!items.length) {
    resultsEl.innerHTML = '<p class="hint">没有找到结果</p>';
  } else {
    resultsEl.innerHTML = items.map(cardHtml).join('');
    const cards = resultsEl.querySelectorAll('.card');
    cards.forEach(function (el, i) { bindCardActions(el, items[i]); });
  }
  const pg = data.pagination || {};
  const total = pg.total || 0;
  const totalPages = pg.totalPages || 1;
  pageInfo.textContent = total ? ('第 ' + (pg.page || currentPage) + ' / ' + totalPages + ' 页，共 ' + total + ' 条') : '';
  prevBtn.disabled = !(pg.page > 1);
  nextBtn.disabled = !pg.hasMore;
}

function infoDl(item) {
  return '<dl class="info">' + infoRows(item).map(function (r) {
    return '<div class="info-row"><dt>' + r[0] + '</dt><dd>' + r[1] + '</dd></div>';
  }).join('') + '</dl>';
}

function cardHtml(item) {
  const title = (item.musicNames && item.musicNames[0]) || item.filename || ('#' + item.id);
  const isPrivate = item.source === 'private';
  const snippet = item.matchContext && item.matchContext.snippet;
  return '<article class="card" data-id="' + escapeHtml(item.id) + '">' +
    '<div class="card-head"><span class="title">' + escapeHtml(title) + '</span>' +
    '<span class="badge' + (isPrivate ? ' private' : '') + '">' + (isPrivate ? '私有' : '公开') + '</span></div>' +
    infoDl(item) +
    '<div class="detail-actions">' +
    '<button type="button" class="copy-btn">复制信息</button>' +
    '<button type="button" class="download-btn">下载 .ttml</button>' +
    '<button type="button" class="preview-btn">歌词预览</button>' +
    '</div>' +
    (snippet ? '<pre class="snippet">' + escapeHtmlKeepMark(snippet) + '</pre>' : '') +
    '<pre class="ttml" hidden></pre>' +
    '</article>';
}

// 按需拉取歌词原文（下载 / 预览用），带缓存
async function fetchDetail(id) {
  if (detailCache.has(id)) return detailCache.get(id);
  const resp = await fetch('/api/v1/lyrics/get?id=' + encodeURIComponent(id));
  const json = await resp.json();
  if (json.status !== 200) throw new Error(json.message || ('status=' + json.status));
  const data = {
    lyrics: json.data.lyrics || '',
    filename: json.data.filename || (id + '.ttml'),
  };
  detailCache.set(id, data);
  return data;
}

function infoRows(data) {
  const rows = [];
  const names = (data.musicNames || []).filter(Boolean);
  const artists = (data.artistNames || []).filter(Boolean);
  const albums = (data.albumNames || []).filter(Boolean);
  const authors = (data.authorUsernames || []).filter(Boolean);
  if (names.length) rows.push(['歌名', escapeHtml(names.join(' / '))]);
  if (artists.length) rows.push(['歌手', escapeHtml(artists.join(' / '))]);
  if (albums.length) rows.push(['专辑', escapeHtml(albums.join(' / '))]);
  if (authors.length) {
    rows.push(['制作者', authors.map(function (u) {
      return '<a class="author-link" href="https://github.com/' + encodeURIComponent(u) +
        '" target="_blank" rel="noopener">' + escapeHtml(u) + '</a>';
    }).join('、')]);
  }
  rows.push(['文件名', escapeHtml(data.filename || '')]);
  return rows;
}

function bindCardActions(cardEl, item) {
  const id = cardEl.getAttribute('data-id');
  const copyBtn = cardEl.querySelector('.copy-btn');
  const dlBtn = cardEl.querySelector('.download-btn');
  const pvBtn = cardEl.querySelector('.preview-btn');
  const pre = cardEl.querySelector('.ttml');

  copyBtn.addEventListener('click', function () {
    const lines = infoRows(item).map(function (r) {
      // 去掉制作者行里的 HTML 标签再复制
      const tmp = document.createElement('div');
      tmp.innerHTML = r[1];
      return r[0] + '：' + tmp.textContent;
    });
    const text = lines.join('\n');
    function done() {
      copyBtn.textContent = '已复制';
      setTimeout(function () { copyBtn.textContent = '复制信息'; }, 1500);
    }
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(done).catch(function () { fallbackCopy(text); done(); });
    } else {
      fallbackCopy(text);
      done();
    }
  });

  dlBtn.addEventListener('click', async function () {
    dlBtn.disabled = true;
    try {
      const data = await fetchDetail(id);
      const blob = new Blob([data.lyrics], { type: 'application/xml' });
      const a = document.createElement('a');
      a.href = URL.createObjectURL(blob);
      a.download = data.filename;
      document.body.appendChild(a);
      a.click();
      setTimeout(function () { URL.revokeObjectURL(a.href); a.remove(); }, 100);
    } catch (e) {
      dlBtn.textContent = '下载失败';
      setTimeout(function () { dlBtn.textContent = '下载 .ttml'; }, 1500);
    }
    dlBtn.disabled = false;
  });

  pvBtn.addEventListener('click', async function () {
    if (!pre.hidden) {
      pre.hidden = true;
      pvBtn.textContent = '歌词预览';
      return;
    }
    if (!pre.dataset.loaded) {
      pvBtn.disabled = true;
      pvBtn.textContent = '加载中…';
      try {
        const data = await fetchDetail(id);
        pre.textContent = data.lyrics || '（无歌词内容）';
        pre.dataset.loaded = '1';
      } catch (e) {
        pre.textContent = '加载失败：' + e.message;
      }
      pvBtn.disabled = false;
    }
    pre.hidden = false;
    pvBtn.textContent = '收起预览';
  });
}

function fallbackCopy(text) {
  const ta = document.createElement('textarea');
  ta.value = text;
  ta.style.position = 'fixed';
  ta.style.opacity = '0';
  document.body.appendChild(ta);
  ta.select();
  try { document.execCommand('copy'); } catch (e) { /* ignore */ }
  ta.remove();
}

form.addEventListener('submit', (ev) => {
  ev.preventDefault();
  const p = readFormParams();
  const hasKeyword = ['q', 'musicName', 'artistName', 'albumName', 'lyricText'].some((k) => p[k]);
  if (!hasKeyword) {
    showError('请至少填写一项搜索条件');
    return;
  }
  currentParams = p;
  detailCache = new Map();
  doSearch(1);
});

prevBtn.addEventListener('click', () => {
  if (currentParams && currentPage > 1) doSearch(currentPage - 1);
});
nextBtn.addEventListener('click', () => {
  if (currentParams && !nextBtn.disabled) doSearch(currentPage + 1);
});

// URL ?q= 预填并自动搜索（供管理页"按文件名搜索确认"跳转）
(function initFromQuery() {
  const q = new URLSearchParams(location.search).get('q');
  if (q) {
    document.getElementById('q').value = q;
    form.requestSubmit();
  }
})();
