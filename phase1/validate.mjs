// CowPaper 阶段一：论文获取验证工具（v2：30天 + 近一年 双窗口）
// 对每本期刊验证 Crossref / OpenAlex / RSS 覆盖，产出结构化报告。
// 运行：node phase1/validate.mjs
import { readFile, writeFile } from 'node:fs/promises';

const CONFIG = JSON.parse(await readFile(new URL('./journals.json', import.meta.url), 'utf8'));
const MAILTO = CONFIG.mailto || 'dev@cowpaper.local';
const UA = `CowPaper-Phase1/0.2 (mailto:${MAILTO})`;
const LOOKBACK = CONFIG.lookbackDays ?? 30;

function fmt(d) {
  const y = d.getUTCFullYear();
  const m = String(d.getUTCMonth() + 1).padStart(2, '0');
  const dd = String(d.getUTCDate()).padStart(2, '0');
  return `${y}-${m}-${dd}`;
}
const TO = fmt(new Date());
const FROM_30 = fmt(new Date(Date.now() - LOOKBACK * 24 * 3600 * 1000));
const FROM_1Y = fmt(new Date(Date.now() - 365 * 24 * 3600 * 1000));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function fetchJSON(url, timeoutMs = 30000) {
  const ctrl = new AbortController();
  const t = setTimeout(() => ctrl.abort(), timeoutMs);
  try {
    const res = await fetch(url, { headers: { 'User-Agent': UA }, signal: ctrl.signal });
    const text = await res.text();
    let data = null;
    try { data = JSON.parse(text); } catch {}
    return { status: res.status, data };
  } catch (e) {
    return { status: 0, error: e.name + ': ' + e.message };
  } finally {
    clearTimeout(t);
  }
}

function normDoi(d) {
  if (!d) return null;
  let s = String(d).trim().toLowerCase();
  s = s.replace(/^https?:\/\/(dx\.)?doi\.org\//i, '');
  s = s.replace(/^doi:\s*/i, '');
  s = s.split('?')[0].split('#')[0].trim();
  return s || null;
}
const hasAbstractInv = (inv) => !!(inv && typeof inv === 'object' && Object.keys(inv).length > 0);
const abs = (items, pick) => {
  const n = items.filter((it) => {
    const v = pick(it);
    return v != null && String(v).trim().length > 0;
  }).length;
  return items.length ? n / items.length : null;
};

async function crossrefWorks(issn, from, to) {
  const url = `https://api.crossref.org/journals/${issn}/works` +
    `?filter=from-pub-date:${from},until-pub-date:${to}` +
    `&sort=published&order=desc&rows=100&select=DOI,title,author,published,issued,abstract,container-title,type&mailto=${MAILTO}`;
  const r = await fetchJSON(url);
  const items = (r.status === 200 && r.data?.message) ? (r.data.message.items || []) : [];
  const total = (r.status === 200 && r.data?.message) ? (r.data.message['total-results'] ?? null) : null;
  return { status: r.status, items, total, error: r.error };
}

async function openAlexWorks(sourceId, from, to) {
  const url = `https://api.openalex.org/works` +
    `?filter=primary_location.source.id:${sourceId},from_publication_date:${from},to_publication_date:${to}` +
    `&sort=publication_date:desc&per-page=100&select=id,doi,title,publication_date,abstract_inverted_index&mailto=${MAILTO}`;
  const r = await fetchJSON(url);
  const items = (r.status === 200 && r.data) ? (r.data.results || []) : [];
  const total = (r.status === 200 && r.data?.meta) ? (r.data.meta.count ?? null) : null;
  return { status: r.status, items, total, error: r.error };
}

async function checkRSS(url) {
  const ctrl = new AbortController();
  const t = setTimeout(() => ctrl.abort(), 20000);
  try {
    const res = await fetch(url, {
      headers: { 'User-Agent': UA, Accept: 'application/rss+xml, application/atom+xml, application/xml, text/xml, */*' },
      signal: ctrl.signal,
    });
    const text = await res.text();
    const head = text.slice(0, 4000);
    const ct = res.headers.get('content-type') || '';
    const looksFeed = /<rss\b|<feed\b|<rdf:RDF/i.test(head) || /application\/(rss|atom)\+xml/i.test(ct);
    return { url, status: res.status, looksFeed, contentType: ct.split(';')[0].trim() || null };
  } catch (e) {
    return { url, status: 0, error: e.name + ': ' + e.message };
  } finally {
    clearTimeout(t);
  }
}

async function validateJournal(j) {
  const out = {
    name: j.name,
    printISSN: j.printISSN,
    onlineISSN: j.onlineISSN,
    publisherHint: j.publisherHint,
    crossref: null,
    openalex: null,
    rss: [],
    hasRSS: false,
    doiIntersection: null,
    provisionalStatus: 'unknown',
  };

  // 1) Crossref 期刊元数据
  const meta = await fetchJSON(`https://api.crossref.org/journals/${j.printISSN}`);
  if (meta.status === 200 && meta.data?.message) {
    const m = meta.data.message;
    out.crossref = {
      title: m.title ?? null,
      publisher: m.publisher ?? null,
      issnType: m['issn-type'] ?? null,
      status: meta.status,
    };
  } else {
    out.crossref = { status: meta.status, error: meta.error ?? 'no message' };
  }
  await sleep(300);

  // 2) Crossref works：30 天 + 近一年
  const cx30 = await crossrefWorks(j.printISSN, FROM_30, TO);
  await sleep(300);
  const cx1y = await crossrefWorks(j.printISSN, FROM_1Y, TO);
  await sleep(300);

  out.crossref.recent30 = {
    total: cx30.total, sampleCount: cx30.items.length,
    abstractCoverage: abs(cx30.items, (it) => it.abstract),
    fieldCoverage: {
      title: abs(cx30.items, (it) => (Array.isArray(it.title) && it.title.length ? it.title[0] : null)),
      author: abs(cx30.items, (it) => (Array.isArray(it.author) && it.author.length ? 'x' : null)),
      date: abs(cx30.items, (it) => (it.published || it.issued)?.date),
      doi: abs(cx30.items, (it) => it.DOI),
    },
  };
  out.crossref.year = {
    total: cx1y.total, sampleCount: cx1y.items.length,
    abstractCoverage: abs(cx1y.items, (it) => it.abstract),
  };
  const dates = cx30.items.map((it) => (it.published || it.issued)?.date).filter(Boolean);
  out.crossref.maxRecentDate = dates.length ? dates.sort().slice(-1)[0] : null;

  // 3) OpenAlex Source
  const src = await fetchJSON(`https://api.openalex.org/sources?filter=issn:${j.printISSN}&mailto=${MAILTO}`);
  let sourceId = null;
  if (src.status === 200 && src.data?.results?.length) {
    const s = src.data.results[0];
    sourceId = s.id.replace('https://openalex.org/', '');
    out.openalex = { id: s.id, displayName: s.display_name ?? null, worksCount: s.works_count ?? null, status: src.status };
  } else {
    out.openalex = { status: src.status, error: src.error ?? 'no results' };
  }
  await sleep(300);

  // 4) OpenAlex works：30 天 + 近一年
  let oa30 = { items: [], total: null };
  let oa1y = { items: [], total: null };
  if (sourceId) {
    oa30 = await openAlexWorks(sourceId, FROM_30, TO);
    await sleep(300);
    oa1y = await openAlexWorks(sourceId, FROM_1Y, TO);
    await sleep(300);
  }
  out.openalex.recent30 = { total: oa30.total, sampleCount: oa30.items.length, abstractCoverage: abs(oa30.items, (it) => (hasAbstractInv(it.abstract_inverted_index) ? 'x' : null)) };
  out.openalex.year = { total: oa1y.total, sampleCount: oa1y.items.length, abstractCoverage: abs(oa1y.items, (it) => (hasAbstractInv(it.abstract_inverted_index) ? 'x' : null)) };

  // 5) DOI 交集（30 天窗口）
  const cxDois = new Set(cx30.items.map((it) => normDoi(it.DOI)).filter(Boolean));
  const oaDois = new Set(oa30.items.map((it) => normDoi(it.doi)).filter(Boolean));
  out.doiIntersection = {
    crossrefDoiCount: cxDois.size,
    openalexDoiCount: oaDois.size,
    intersectionCount: [...cxDois].filter((x) => oaDois.has(x)).length,
  };

  // 6) RSS
  for (const url of j.rssCandidates || []) {
    out.rss.push(await checkRSS(url));
    await sleep(200);
  }
  out.hasRSS = out.rss.some((r) => r.status === 200 && r.looksFeed);

  // 7) 支持状态（用「近一年」判断是否可发现论文，用最佳年度摘要覆盖判断摘要质量）
  const yearWorks = (out.crossref.year?.total ?? 0) + (out.openalex.year?.total ?? 0);
  const bestYearAbs = Math.max(out.crossref.year?.abstractCoverage ?? 0, out.openalex.year?.abstractCoverage ?? 0);
  if (yearWorks === 0) out.provisionalStatus = 'unsupported';
  else if (bestYearAbs >= 0.7) out.provisionalStatus = 'fullySupported';
  else if (bestYearAbs > 0) out.provisionalStatus = 'supportedWithMissingAbstracts';
  else out.provisionalStatus = 'adapterRequired';

  return out;
}

const results = [];
for (const j of CONFIG.journals) {
  process.stdout.write(`\n== ${j.name} (${j.printISSN}) ==\n`);
  const r = await validateJournal(j);
  results.push(r);
  const c = r.crossref, o = r.openalex;
  process.stdout.write(
    `  Crossref 30d=${c.recent30.total ?? '?'} 1y=${c.year.total ?? '?'} | 摘要(1y)=${pct(c.year.abstractCoverage)}\n` +
    `  OpenAlex 30d=${o.recent30.total ?? '?'} 1y=${o.year.total ?? '?'} | 摘要(1y)=${pct(o.year.abstractCoverage)}\n` +
    `  RSS=${r.rss.map((x) => `${x.status}${x.looksFeed ? '✓' : ''}`).join(',')} | 状态=${r.provisionalStatus}\n`,
  );
  await sleep(400);
}
function pct(x) { return x == null ? 'N/A' : (x * 100).toFixed(0) + '%'; }

const report = {
  generatedAt: new Date().toISOString(),
  window: { from30: FROM_30, from1y: FROM_1Y, to: TO },
  mailto: MAILTO,
  journals: results,
};
await writeFile(new URL('./report.json', import.meta.url), JSON.stringify(report, null, 2));
process.stdout.write('\n[完成] 报告已写入 phase1/report.json\n');
