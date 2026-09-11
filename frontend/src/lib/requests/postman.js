// Postman import. Handles Collection v2.0/v2.1 and Environment exports.
// Postman's {{var}} template syntax already matches OG TestDesk's, so
// variables carry over unchanged.

function urlToString(url) {
  if (!url) return '';
  if (typeof url === 'string') return url;
  if (url.raw) return url.raw;
  const proto = url.protocol ? url.protocol + '://' : '';
  const host = Array.isArray(url.host) ? url.host.join('.') : url.host || '';
  const path = Array.isArray(url.path) ? url.path.join('/') : url.path || '';
  const q = Array.isArray(url.query)
    ? url.query
        .filter((x) => !x.disabled)
        .map((x) => `${x.key}=${x.value ?? ''}`)
        .join('&')
    : '';
  return `${proto}${host}${path ? '/' + path : ''}${q ? '?' + q : ''}`;
}

function headersToObject(header) {
  const out = {};
  if (Array.isArray(header)) {
    for (const h of header) if (h && h.key && !h.disabled) out[h.key] = h.value ?? '';
  }
  return out;
}

function bodyToString(body) {
  if (!body) return null;
  if (body.mode === 'raw') return body.raw ?? null;
  if (body.mode === 'urlencoded' && Array.isArray(body.urlencoded)) {
    return body.urlencoded
      .filter((x) => !x.disabled)
      .map((x) => `${encodeURIComponent(x.key)}=${encodeURIComponent(x.value ?? '')}`)
      .join('&');
  }
  if (body.mode === 'formdata' && Array.isArray(body.formdata)) {
    // can't do real multipart here — flatten to JSON so nothing is lost
    return JSON.stringify(
      Object.fromEntries(body.formdata.filter((x) => !x.disabled).map((x) => [x.key, x.value ?? ''])),
      null,
      2
    );
  }
  if (body.mode === 'graphql' && body.graphql) {
    return JSON.stringify({ query: body.graphql.query, variables: body.graphql.variables }, null, 2);
  }
  return body.raw ?? null;
}

/**
 * @returns {{ kind:'collection', name:string, requests:Array<{name,method,url,headers,body,folder}> }}
 */
export function parsePostmanCollection(json) {
  const name = json?.info?.name || 'Imported collection';
  const requests = [];

  const walk = (items, folder) => {
    if (!Array.isArray(items)) return;
    for (const it of items) {
      if (it.item) {
        walk(it.item, folder ? `${folder} / ${it.name}` : it.name);
      } else if (it.request) {
        const r = it.request;
        requests.push({
          name: it.name || 'Request',
          method: (typeof r === 'string' ? 'GET' : r.method || 'GET').toUpperCase(),
          url: urlToString(typeof r === 'string' ? r : r.url),
          headers: headersToObject(r.header),
          body: bodyToString(r.body),
          folder: folder || null
        });
      }
    }
  };
  walk(json?.item, null);
  return { kind: 'collection', name, requests };
}

/**
 * @returns {{ kind:'environment', name:string, variables:Record<string,string> }}
 */
export function parsePostmanEnvironment(json) {
  const variables = {};
  for (const v of json?.values || []) {
    if (v && v.key && v.enabled !== false) variables[v.key] = String(v.value ?? '');
  }
  return { kind: 'environment', name: json?.name || 'Imported environment', variables };
}

/** Detect + dispatch. Returns null if it doesn't look like Postman. */
export function parsePostman(text) {
  let json;
  try {
    json = JSON.parse(text);
  } catch {
    return null;
  }
  if (json?.info?.schema?.includes('collection') || Array.isArray(json?.item)) {
    return parsePostmanCollection(json);
  }
  if (Array.isArray(json?.values) && (json?._postman_variable_scope || json?.name)) {
    return parsePostmanEnvironment(json);
  }
  return null;
}

// ---------------------------------------------------------------- export

function headersToArray(headersJson) {
  let obj;
  try {
    obj = JSON.parse(headersJson || '{}');
  } catch {
    obj = {};
  }
  return Object.entries(obj).map(([key, value]) => ({ key, value: String(value ?? '') }));
}

function toPostmanRequest(r) {
  const item = {
    name: r.name,
    request: {
      method: r.method,
      header: headersToArray(r.headers_json),
      url: { raw: r.url }
    }
  };
  if (r.body) {
    item.request.body = { mode: 'raw', raw: r.body };
  }
  return item;
}

/**
 * Build a Postman Collection v2.1 JSON object from our collections
 * (`{id, name, parent_id}`) + saved requests (`{collection_id, ...}`).
 * Nesting follows parent_id; requests with no collection_id land at the
 * root alongside the top-level folders.
 */
export function toPostmanCollection(name, collections, requests) {
  const byParent = new Map(); // parent_id (or null) -> collection[]
  for (const c of collections) {
    const key = c.parent_id || null;
    if (!byParent.has(key)) byParent.set(key, []);
    byParent.get(key).push(c);
  }
  const reqsByCollection = new Map();
  for (const r of requests) {
    const key = r.collection_id || null;
    if (!reqsByCollection.has(key)) reqsByCollection.set(key, []);
    reqsByCollection.get(key).push(r);
  }

  const buildFolder = (collectionId) => {
    const subfolders = (byParent.get(collectionId) || []).map((c) => ({
      name: c.name,
      item: buildFolder(c.id)
    }));
    const ownRequests = (reqsByCollection.get(collectionId) || []).map(toPostmanRequest);
    return [...subfolders, ...ownRequests];
  };

  return {
    info: {
      name: name || 'OG TestDesk export',
      schema: 'https://schema.getpostman.com/json/collection/v2.1.0/collection.json'
    },
    item: buildFolder(null)
  };
}

/** Build a Postman Environment JSON object from a plain {key: value} map. */
export function toPostmanEnvironment(name, variables) {
  return {
    name: name || 'Environment',
    values: Object.entries(variables || {}).map(([key, value]) => ({
      key,
      value: String(value ?? ''),
      enabled: true
    })),
    _postman_variable_scope: 'environment'
  };
}
