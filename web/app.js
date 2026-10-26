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
    for (const el of resultsEl.querySelectorAll('.card')) {
      el.addEventListener('click', (ev) => {
        if (!ev.target.closest('.detail, .platform-ids')) toggleDetail(el);
      });
    }
  }
  const pg = data.pagination || {};
  const total = pg.total || 0;
  const totalPages = pg.totalPages || 1;
  pageInfo.textContent = total ? ('第 ' + (pg.page || currentPage) + ' / ' + totalPages + ' 页，共 ' + total + ' 条') : '';
  prevBtn.disabled = !(pg.page > 1);
  nextBtn.disabled = !pg.hasMore;
}

function cardHtml(item) {
  const title = (item.musicNames && item.musicNames[0]) || item.filename || ('#' + item.id);
  const artists = (item.artistNames || []).join(' / ');
  const albums = (item.albumNames || []).join(' / ');
  const isPrivate = item.source === 'private';
  const snippet = item.matchContext && item.matchContext.snippet;
  const platformIds = [
    ['网易云音乐 ID', item.ncmMusicIds],
    ['QQ 音乐 ID', item.qqMusicIds],
    ['Apple Music ID', item.appleMusicIds],
    ['Spotify ID', item.spotifyIds],
    ['ISRC', item.isrcs]
  ].filter(([, ids]) => ids && ids.length)
    .map(([label, ids]) => '<div class="meta dim">' + label + '：' + escapeHtml(ids.join(' / ')) + '</div>')
    .join('');
  return '<article class="card" data-id="' + escapeHtml(item.id) + '">' +
    '<div class="card-head"><span class="title">' + escapeHtml(title) + '</span>' +
    '<span class="badge' + (isPrivate ? ' private' : '') + '">' + (isPrivate ? '私有' : '公开') + '</span></div>' +
    (artists ? '<div class="meta">歌手：' + escapeHtml(artists) + '</div>' : '') +
    (albums ? '<div class="meta">专辑：' + escapeHtml(albums) + '</div>' : '') +
    '<div class="meta dim">' + escapeHtml(item.filename || '') + '</div>' +
    (platformIds ? '<div class="platform-ids" aria-label="平台 ID">' + platformIds + '</div>' : '') +
    (snippet ? '<pre class="snippet">' + escapeHtmlKeepMark(snippet) + '</pre>' : '') +
    '<div class="detail" hidden></div>' +
    '</article>';
}

async function toggleDetail(cardEl) {
  const detailEl = cardEl.querySelector('.detail');
  const id = cardEl.getAttribute('data-id');
  if (!detailEl.hidden) {
    detailEl.hidden = true;
    detailEl.innerHTML = '';
    return;
  }
  detailEl.hidden = false;
  if (detailCache.has(id)) {
    detailEl.innerHTML = detailHtml(detailCache.get(id));
    bindDownload(detailEl, detailCache.get(id));
    return;
  }
  detailEl.innerHTML = '<p class="hint">加载中…</p>';
  try {
    const resp = await fetch('/api/v1/lyrics/get?id=' + encodeURIComponent(id));
    const json = await resp.json();
    if (json.status !== 200) throw new Error(json.message || ('status=' + json.status));
    const data = { lyrics: json.data.lyrics || '', filename: json.data.filename || (id + '.ttml') };
    detailCache.set(id, data);
    detailEl.innerHTML = detailHtml(data);
    bindDownload(detailEl, data);
  } catch (e) {
    detailEl.innerHTML = '<p class="error">加载失败：' + escapeHtml(e.message) + '</p>';
  }
}

function detailHtml(data) {
  return '<pre class="ttml"><code>' + escapeHtml(data.lyrics) + '</code></pre>' +
    '<button type="button" class="download-btn">下载 .ttml</button>';
}

function bindDownload(detailEl, data) {
  const btn = detailEl.querySelector('.download-btn');
  btn.addEventListener('click', (ev) => {
    ev.stopPropagation();
    const blob = new Blob([data.lyrics], { type: 'application/xml' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = data.filename;
    document.body.appendChild(a);
    a.click();
    setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 100);
  });
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
