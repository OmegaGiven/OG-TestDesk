// Browser fallback: when the page is opened outside the Tauri webview
// (plain `pnpm dev` in a browser, or headless screenshotting) there is no
// native IPC. This installs a small in-memory backend so the UI is
// navigable and demoable without the Rust side. No-op inside Tauri.

if (typeof window !== 'undefined' && !window.__TAURI_INTERNALS__) {
  const DEMO_SHOP = 'conn-demo-shop';
  const LOCAL_PG = 'conn-local-pg';

  const connections = [
    {
      id: DEMO_SHOP,
      nickname: 'DEMO SHOP',
      kind: 'sqlite',
      host: null,
      port: null,
      database: null,
      user: null,
      file_path: '~/projects/OG-TestDesk-tauri/demo/shop.db',
      use_tls: false,
      color: 'var(--conn-teal)'
    },
    {
      id: LOCAL_PG,
      nickname: 'LOCAL PG',
      kind: 'postgres',
      host: 'localhost',
      port: 5432,
      database: 'postgres',
      user: 'postgres',
      file_path: null,
      use_tls: false,
      color: 'var(--conn-blue)'
    }
  ];

  const tabs = [
    {
      id: 'tab-1',
      connection_id: DEMO_SHOP,
      title: 'Top customers',
      sql_text: 'SELECT * FROM customer_revenue LIMIT 15;',
      position: 0,
      is_active: true
    },
    {
      id: 'tab-2',
      connection_id: DEMO_SHOP,
      title: 'Orders by status',
      sql_text:
        'SELECT status, COUNT(*) AS n, ROUND(SUM(total),2) AS revenue\nFROM orders GROUP BY status ORDER BY revenue DESC;',
      position: 1,
      is_active: false
    },
    {
      id: 'tab-3',
      connection_id: DEMO_SHOP,
      title: 'Order detail',
      sql_text:
        'SELECT o.id, c.name, o.status, o.placed_at, oi.qty, p.name AS product, oi.unit_price\nFROM orders o\nJOIN customers c ON c.id = o.customer_id\nJOIN order_items oi ON oi.order_id = o.id\nJOIN products p ON p.id = oi.product_id\nORDER BY o.id DESC LIMIT 100;',
      position: 2,
      is_active: false
    },
    {
      id: 'tab-4',
      connection_id: DEMO_SHOP,
      title: 'By city + status',
      sql_text:
        "SELECT c.city, o.status, COUNT(*) AS orders, ROUND(SUM(o.total),2) AS revenue\nFROM orders o JOIN customers c ON c.id = o.customer_id\nWHERE c.city = '{{city}}' AND o.status = '{{status}}'\nGROUP BY c.city, o.status;",
      position: 3,
      is_active: false
    }
  ];

  const schemas = [
    {
      name: 'main',
      relations: [
        { name: 'customers', kind: 'table' },
        { name: 'order_items', kind: 'table' },
        { name: 'orders', kind: 'table' },
        { name: 'products', kind: 'table' },
        { name: 'customer_revenue', kind: 'view' }
      ]
    }
  ];

  const columns = {
    customers: [
      { name: 'id', data_type: 'INTEGER', nullable: false, primary_key: true, default: null },
      { name: 'name', data_type: 'TEXT', nullable: false, primary_key: false, default: null },
      { name: 'email', data_type: 'TEXT', nullable: true, primary_key: false, default: null },
      { name: 'city', data_type: 'TEXT', nullable: true, primary_key: false, default: null },
      { name: 'created_at', data_type: 'TEXT', nullable: true, primary_key: false, default: null },
      { name: 'lifetime_value', data_type: 'REAL', nullable: true, primary_key: false, default: null },
      { name: 'is_active', data_type: 'BOOLEAN', nullable: true, primary_key: false, default: null }
    ],
    orders: [
      { name: 'id', data_type: 'INTEGER', nullable: false, primary_key: true, default: null },
      { name: 'customer_id', data_type: 'INTEGER', nullable: true, primary_key: false, default: null },
      { name: 'status', data_type: 'TEXT', nullable: true, primary_key: false, default: null },
      { name: 'placed_at', data_type: 'TEXT', nullable: true, primary_key: false, default: null },
      { name: 'total', data_type: 'REAL', nullable: true, primary_key: false, default: null },
      { name: 'notes', data_type: 'TEXT', nullable: true, primary_key: false, default: null }
    ]
  };

  const CITIES = ['Dallas', 'Austin', 'Denver', 'Portland', 'Chicago', 'Miami', 'Seattle'];
  const NAMES = ['Ava Ng', 'Liam Patel', 'Noah Reyes', 'Emma Kim', 'Oliver Okafor', 'Mia Silva',
    'Ethan Larsen', 'Sophia Haddad', 'Lucas Cruz', 'Zoe Bauer', 'Mason Ng', 'Isla Patel',
    'Ava Reyes', 'Liam Kim', 'Noah Silva'];
  const topCustomers = {
    columns: [
      { name: 'id', type_name: 'INTEGER' },
      { name: 'name', type_name: 'TEXT' },
      { name: 'city', type_name: 'TEXT' },
      { name: 'orders', type_name: 'INTEGER' },
      { name: 'revenue', type_name: 'REAL' }
    ],
    rows: NAMES.map((n, i) => [
      i + 1,
      n,
      CITIES[i % CITIES.length],
      Math.floor(Math.random() * 4) + 1,
      Math.round((4200 - i * 230 + Math.random() * 90) * 100) / 100
    ]),
    row_count: 15,
    rows_affected: 0,
    duration_ms: 3,
    is_select: true
  };
  const ordersByStatus = {
    columns: [
      { name: 'status', type_name: 'TEXT' },
      { name: 'n', type_name: 'INTEGER' },
      { name: 'revenue', type_name: 'REAL' }
    ],
    rows: [
      ['delivered', 21, 18422.55],
      ['paid', 17, 14201.10],
      ['shipped', 15, 12980.40],
      ['pending', 12, 9004.22],
      ['refunded', 10, 7411.98]
    ],
    row_count: 5,
    rows_affected: 0,
    duration_ms: 2,
    is_select: true
  };

  const environments = [
    {
      id: 'env-demo',
      name: 'Demo APIs',
      variables_json: JSON.stringify({
        baseUrl: 'https://jsonplaceholder.typicode.com',
        httpbin: 'https://httpbin.org'
      }),
      is_active: true
    }
  ];
  const collections = [{ id: 'col-play', name: 'Playground', parent_id: null }];
  const savedRequests = [
    { id: 'r1', collection_id: 'col-play', name: 'List posts', method: 'GET', url: '{{baseUrl}}/posts', headers_json: '{}', body: null, sort_order: 0, created_at: 0 },
    { id: 'r2', collection_id: 'col-play', name: 'Get post 1', method: 'GET', url: '{{baseUrl}}/posts/1', headers_json: '{}', body: null, sort_order: 1, created_at: 0 },
    { id: 'r3', collection_id: 'col-play', name: 'Create post', method: 'POST', url: '{{baseUrl}}/posts', headers_json: '{"Content-Type":"application/json"}', body: '{\n  "title": "hello",\n  "body": "from OG TestDesk",\n  "userId": 1\n}', sort_order: 2, created_at: 0 },
    { id: 'r4', collection_id: 'col-play', name: 'Headers echo', method: 'GET', url: '{{httpbin}}/headers', headers_json: '{"X-Demo":"true"}', body: null, sort_order: 3, created_at: 0 }
  ];

  const state = { 'sqlvars:tab-4': JSON.stringify({ city: 'Dallas', status: 'paid' }) };
  const ok = (v) => Promise.resolve(v);

  const handlers = {
    connections_list: () => ok(connections),
    connection_save: ({ config }) => ok({ ...config, id: config.id || 'conn-' + Date.now() }),
    connection_delete: () => ok(null),
    connections_reorder: () => ok(null),
    connection_test: ({ config }) =>
      ok({ kind: config.kind, version: config.kind === 'sqlite' ? '3.45.0' : '16.2 (mock)' }),
    schemas_list: () => ok(schemas),
    columns_list: ({ relation }) => ok(columns[relation] || []),
    query_run: ({ sql }) => {
      const s = (sql || '').toLowerCase();
      if (s.includes('group by c.city')) {
        const city = (sql.match(/c\.city = '([^']*)'/) || [])[1] || 'Dallas';
        const status = (sql.match(/o\.status = '([^']*)'/) || [])[1] || 'paid';
        return ok({
          columns: [
            { name: 'city', type_name: 'TEXT' },
            { name: 'status', type_name: 'TEXT' },
            { name: 'orders', type_name: 'INTEGER' },
            { name: 'revenue', type_name: 'REAL' }
          ],
          rows: [[city, status, 6, 4188.4]],
          row_count: 1,
          rows_affected: 0,
          duration_ms: 2,
          is_select: true
        });
      }
      if (s.includes('group by status')) return ok(ordersByStatus);
      if (s.includes('customer_revenue')) return ok(topCustomers);
      return ok({ ...topCustomers, duration_ms: 4 });
    },
    tabs_list_all: () => ok(tabs),
    tab_save: ({ tab }) => ok({ ...tab, id: tab.id || 'tab-' + Date.now() }),
    tab_delete: () => ok(null),
    history_recent: () => ok([]),
    saved_queries_list: () => ok([]),
    saved_query_save: ({ query }) => ok({ ...query, id: query.id || 'sq-' + Date.now() }),
    saved_query_delete: () => ok(null),
    collections_list: () => ok(collections),
    collection_save: ({ collection }) => ok({ ...collection, id: collection.id || 'col-' + Date.now() }),
    collection_delete: () => ok(null),
    saved_requests_list: () => ok(savedRequests),
    saved_request_save: ({ request }) => ok({ ...request, id: request.id || 'r-' + Date.now() }),
    saved_request_delete: () => ok(null),
    environments_list: () => ok(environments),
    environment_save: ({ environment }) => ok({ ...environment, id: environment.id || 'env-' + Date.now() }),
    environment_delete: () => ok(null),
    request_send: ({ request }) =>
      ok({
        status: request.method === 'POST' ? 201 : 200,
        status_text: request.method === 'POST' ? 'Created' : 'OK',
        headers: [
          ['content-type', 'application/json; charset=utf-8'],
          ['x-powered-by', 'Express'],
          ['cache-control', 'no-cache']
        ],
        body: JSON.stringify(
          request.method === 'POST'
            ? { id: 101, title: 'hello', body: 'from OG TestDesk', userId: 1 }
            : { userId: 1, id: 1, title: 'sunt aut facere repellat provident', body: 'quia et suscipit\nsuscipit recusandae' },
          null,
          2
        ),
        content_type: 'application/json; charset=utf-8',
        is_json: true,
        duration_ms: 128,
        size_bytes: 292
      }),
    state_get: ({ key }) => ok(state[key] ?? null),
    state_set: ({ key, value }) => {
      state[key] = value;
      return ok(null);
    }
  };

  window.__TAURI_INTERNALS__ = {
    transformCallback: (cb) => cb,
    invoke: (cmd, args) =>
      handlers[cmd]
        ? handlers[cmd](args || {})
        : Promise.reject(`mock: no handler for ${cmd}`)
  };
  console.info('[OG TestDesk] mock IPC installed (not running in Tauri)');
}
