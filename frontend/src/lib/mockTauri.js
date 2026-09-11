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
    },
    {
      id: 'tab-5',
      connection_id: DEMO_SHOP,
      title: 'Everything',
      sql_text: 'SELECT * FROM order_items;',
      position: 4,
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
    foreign_keys_list: () =>
      ok([
        { schema: 'main', table: 'orders', column: 'customer_id', ref_schema: 'main', ref_table: 'customers', ref_column: 'id' },
        { schema: 'main', table: 'order_items', column: 'order_id', ref_schema: 'main', ref_table: 'orders', ref_column: 'id' },
        { schema: 'main', table: 'order_items', column: 'product_id', ref_schema: 'main', ref_table: 'products', ref_column: 'id' }
      ]),
    query_run: ({ sql, page, pageSize, count }) => {
      const s = (sql || '').toLowerCase();
      const paged = (allRows, cols, total, dur) => {
        if (pageSize && pageSize > 0) {
          const off = (page || 0) * pageSize;
          const slice = allRows.slice(off, off + pageSize);
          return ok({
            columns: cols,
            rows: slice,
            row_count: slice.length,
            rows_affected: 0,
            duration_ms: dur,
            is_select: true,
            truncated: false,
            page: page || 0,
            page_size: pageSize,
            total: count ? total : null,
            count_ms: count ? 4 : null,
            has_more: off + slice.length < total
          });
        }
        const capped = Math.min(allRows.length, mockLimit || 100000);
        return ok({
          columns: cols,
          rows: allRows.slice(0, capped),
          row_count: capped,
          rows_affected: 0,
          duration_ms: dur,
          is_select: true,
          truncated: capped < allRows.length,
          page: 0,
          page_size: 0,
          total: null,
          count_ms: null,
          has_more: false
        });
      };

      if (s.includes('from order_items')) {
        const total = 12384;
        const all = Array.from({ length: total }, (_, i) => [
          i + 1,
          (i % 75) + 1,
          (i % 25) + 1,
          (i % 3) + 1,
          Math.round((9.99 + (i % 40) * 7.5) * 100) / 100
        ]);
        return paged(
          all,
          [
            { name: 'id', type_name: 'INTEGER' },
            { name: 'order_id', type_name: 'INTEGER' },
            { name: 'product_id', type_name: 'INTEGER' },
            { name: 'qty', type_name: 'INTEGER' },
            { name: 'unit_price', type_name: 'REAL' }
          ],
          total,
          41
        );
      }
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
    history_recent: () =>
      ok([
        {
          id: 'h1',
          connection_id: DEMO_SHOP,
          sql_text: 'SELECT * FROM customer_revenue LIMIT 15;',
          duration_ms: 3,
          row_count: 15,
          success: true,
          error: null,
          has_result: true,
          ran_at: Math.floor(Date.now() / 1000) - 120
        },
        {
          id: 'h2',
          connection_id: DEMO_SHOP,
          sql_text: 'SELECT status, COUNT(*) FROM orders GROUP BY status;',
          duration_ms: 2,
          row_count: 5,
          success: true,
          error: null,
          has_result: true,
          ran_at: Math.floor(Date.now() / 1000) - 900
        },
        {
          id: 'h3',
          connection_id: DEMO_SHOP,
          sql_text: 'SELECT * FROM nonexistent;',
          duration_ms: null,
          row_count: null,
          success: false,
          error: 'no such table: nonexistent',
          has_result: false,
          ran_at: Math.floor(Date.now() / 1000) - 3600
        }
      ]),
    history_result: ({ id }) =>
      ok(id === 'h2' ? JSON.stringify(ordersByStatus) : JSON.stringify(topCustomers)),
    history_request_result: () =>
      ok(
        JSON.stringify({
          status: 200,
          status_text: 'OK',
          headers: [['content-type', 'application/json; charset=utf-8']],
          body: JSON.stringify([{ userId: 1, id: 1, title: 'sunt aut facere repellat' }], null, 2),
          content_type: 'application/json; charset=utf-8',
          is_json: true,
          duration_ms: 96,
          size_bytes: 27520
        })
      ),
    query_limits_get: () => ok(mockLimit),
    query_limits_set: ({ maxRows }) => {
      mockLimit = maxRows;
      return ok(null);
    },
    history_request_recent: () =>
      ok([
        {
          id: 'rh1',
          saved_request_id: 'r1',
          name: 'List posts',
          method: 'GET',
          url: 'https://jsonplaceholder.typicode.com/posts',
          headers_json: '{}',
          body: null,
          status: 200,
          duration_ms: 96,
          size_bytes: 27520,
          success: true,
          error: null,
          has_response: true,
          sent_at: Math.floor(Date.now() / 1000) - 240
        },
        {
          id: 'rh2',
          saved_request_id: 'r3',
          name: 'Create post',
          method: 'POST',
          url: 'https://jsonplaceholder.typicode.com/posts',
          headers_json: '{"Content-Type":"application/json"}',
          body: '{ "title": "hello" }',
          status: 201,
          duration_ms: 128,
          size_bytes: 292,
          success: true,
          error: null,
          has_response: false,
          sent_at: Math.floor(Date.now() / 1000) - 1500
        }
      ]),
    schedules_list: () => ok(mockSchedules),
    schedule_save: ({ schedule }) => {
      const s = { ...schedule, id: schedule.id || 'sch-' + Date.now(), next_run: Math.floor(Date.now() / 1000) + 3600 };
      const i = mockSchedules.findIndex((x) => x.id === s.id);
      if (i >= 0) mockSchedules[i] = s;
      else mockSchedules = [...mockSchedules, s];
      return ok(s);
    },
    schedule_delete: ({ id }) => {
      mockSchedules = mockSchedules.filter((s) => s.id !== id);
      return ok(null);
    },
    schedule_run_now: () => ok('ok · 15 rows'),
    saved_queries_list: () => ok(mockSavedQueries),
    saved_query_save: ({ query }) => {
      const s = { ...query, id: query.id || 'sq-' + Date.now() };
      const i = mockSavedQueries.findIndex((x) => x.id === s.id);
      if (i >= 0) mockSavedQueries[i] = s;
      else mockSavedQueries = [...mockSavedQueries, s];
      return ok(s);
    },
    saved_query_delete: ({ id }) => {
      mockSavedQueries = mockSavedQueries.filter((x) => x.id !== id);
      return ok(null);
    },
    saved_charts_list: () => ok(mockSavedCharts.map(({ data_json, ...c }) => ({ ...c, has_data: !!data_json }))),
    saved_chart_data: ({ id }) => ok(mockSavedCharts.find((c) => c.id === id)?.data_json ?? null),
    saved_chart_save: ({ chart }) => {
      const c = { sort_order: 0, ...chart, id: chart.id || 'sc-' + Date.now(), created_at: chart.created_at || Math.floor(Date.now() / 1000) };
      const i = mockSavedCharts.findIndex((x) => x.id === c.id);
      if (i >= 0) mockSavedCharts[i] = { ...mockSavedCharts[i], ...c, data_json: c.data_json ?? mockSavedCharts[i].data_json };
      else mockSavedCharts = [...mockSavedCharts, c];
      const { data_json, ...rest } = c;
      return ok({ ...rest, has_data: !!data_json });
    },
    saved_chart_delete: ({ id }) => {
      mockSavedCharts = mockSavedCharts.filter((x) => x.id !== id);
      return ok(null);
    },
    saved_query_folders_list: () => ok(mockSavedQueryFolders),
    saved_query_folder_save: ({ folder }) => {
      const f = { sort_order: 0, ...folder, id: folder.id || 'sqf-' + Date.now() };
      const i = mockSavedQueryFolders.findIndex((x) => x.id === f.id);
      if (i >= 0) mockSavedQueryFolders[i] = f;
      else mockSavedQueryFolders = [...mockSavedQueryFolders, f];
      return ok(f);
    },
    saved_query_folder_delete: ({ id }) => {
      const gone = new Set([id]);
      let grew = true;
      while (grew) {
        grew = false;
        for (const f of mockSavedQueryFolders)
          if (f.parent_id && gone.has(f.parent_id) && !gone.has(f.id)) {
            gone.add(f.id);
            grew = true;
          }
      }
      mockSavedQueryFolders = mockSavedQueryFolders.filter((x) => !gone.has(x.id));
      mockSavedQueries = mockSavedQueries.map((q) =>
        gone.has(q.folder_id) ? { ...q, folder_id: null } : q
      );
      return ok(null);
    },
    collections_list: () => ok(collections),
    collection_save: ({ collection }) => ok({ ...collection, id: collection.id || 'col-' + Date.now() }),
    collection_delete: () => ok(null),
    saved_requests_list: () => ok(savedRequests),
    saved_request_save: ({ request }) => ok({ ...request, id: request.id || 'r-' + Date.now() }),
    saved_request_delete: () => ok(null),
    request_tabs_list: () => ok(mockReqTabs),
    request_tab_save: ({ tab }) => {
      const s = { ...tab, id: tab.id || 'rt-' + Date.now() };
      const i = mockReqTabs.findIndex((x) => x.id === s.id);
      if (i >= 0) mockReqTabs[i] = s;
      else mockReqTabs = [...mockReqTabs, s];
      return ok(s);
    },
    request_tab_delete: ({ id }) => {
      mockReqTabs = mockReqTabs.filter((t) => t.id !== id);
      return ok(null);
    },
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
    window_environment: () => ok({ tiling: false, os: 'linux' }),
    state_get: ({ key }) => ok(state[key] ?? null),
    state_set: ({ key, value }) => {
      state[key] = value;
      return ok(null);
    },

    // MCP (mock)
    mcp_config_get: () =>
      ok(
        mcpCfg || {
          enabled: true,
          port: 7788,
          token: 'a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6',
          allow_write: false,
          allow_http: true
        }
      ),
    mcp_status: () => ok({ running: (mcpCfg || { enabled: true }).enabled, port: 7788 }),
    mcp_config_set: ({ config }) => {
      mcpCfg = config;
      return ok({ running: config.enabled, port: config.port });
    },
    mcp_start: () => {
      mcpCfg = { ...(mcpCfg || {}), enabled: true };
      return ok({ running: true, port: 7788 });
    },
    mcp_stop: () => {
      mcpCfg = { ...(mcpCfg || {}), enabled: false };
      return ok({ running: false, port: 7788 });
    },
    mcp_acls_get: () => ok(mcpAcls),
    mcp_acl_set: ({ connectionId, acl }) => {
      mcpAcls = { ...mcpAcls, [connectionId]: acl };
      return ok(null);
    }
  };
  let mcpCfg = null;
  let mockLimit = 10000;
  let mockSavedQueryFolders = [
    { id: 'sqf-reports', name: 'Reports', parent_id: null, sort_order: 0 },
    { id: 'sqf-daily', name: 'Daily', parent_id: 'sqf-reports', sort_order: 0 }
  ];
  let mockSavedQueries = [
    {
      id: 'sq-1',
      connection_id: DEMO_SHOP,
      folder_id: 'sqf-reports',
      name: 'Top customers',
      sql_text: 'SELECT * FROM customer_revenue LIMIT 15;',
      sort_order: 0,
      created_at: 0
    },
    {
      id: 'sq-2',
      connection_id: DEMO_SHOP,
      folder_id: 'sqf-daily',
      name: 'Orders by status',
      sql_text: 'SELECT status, COUNT(*) FROM orders GROUP BY status;',
      sort_order: 0,
      created_at: 0
    },
    {
      id: 'sq-3',
      connection_id: DEMO_SHOP,
      folder_id: null,
      name: 'Row counts',
      sql_text:
        "SELECT 'customers' t, COUNT(*) n FROM customers\nUNION ALL SELECT 'orders', COUNT(*) FROM orders;",
      sort_order: 0,
      created_at: 0
    }
  ];
  let mockReqTabs = [
    {
      id: 'rt-1',
      saved_request_id: 'r1',
      title: 'List posts',
      method: 'GET',
      url: '{{baseUrl}}/posts',
      headers_json: '{}',
      body: null,
      position: 0,
      is_active: true
    },
    {
      id: 'rt-2',
      saved_request_id: 'r3',
      title: 'Create post',
      method: 'POST',
      url: '{{baseUrl}}/posts',
      headers_json: '{"Content-Type":"application/json"}',
      body: '{\n  "title": "hello",\n  "body": "from OG TestDesk",\n  "userId": 1\n}',
      position: 1,
      is_active: false
    }
  ];
  let mockSavedCharts = [
    {
      id: 'sc-1',
      name: 'Revenue by status',
      connection_id: DEMO_SHOP,
      saved_query_id: null,
      sql_text: 'SELECT status, COUNT(*) AS n, ROUND(SUM(total),2) AS revenue FROM orders GROUP BY status;',
      chart_type: 'bar',
      x_field: 'status',
      y_fields_json: '["revenue"]',
      options_json: '{}',
      data_json: JSON.stringify([
        { status: 'delivered', n: 21, revenue: 18422.55 },
        { status: 'paid', n: 17, revenue: 14201.1 },
        { status: 'shipped', n: 15, revenue: 12980.4 },
        { status: 'pending', n: 12, revenue: 9004.22 },
        { status: 'refunded', n: 10, revenue: 7411.98 }
      ]),
      row_count: 5,
      last_run_at: Math.floor(Date.now() / 1000) - 3600,
      sort_order: 0,
      created_at: Math.floor(Date.now() / 1000) - 86400
    }
  ];
  let mcpAcls = { [DEMO_SHOP]: { exposed: true, allow_writes: false } };
  let mockSchedules = [
    {
      id: 'sch-1',
      name: 'Hourly revenue snapshot',
      kind: 'sql',
      connection_id: DEMO_SHOP,
      sql_text: 'SELECT status, COUNT(*) FROM orders GROUP BY status;',
      saved_request_id: null,
      request_method: null,
      request_url: null,
      request_headers_json: null,
      request_body: null,
      schedule_expr: 'every:3600',
      enabled: true,
      last_run: Math.floor(Date.now() / 1000) - 1200,
      last_status: 'ok · 5 rows',
      next_run: Math.floor(Date.now() / 1000) + 2400,
      created_at: 0
    },
    {
      id: 'sch-2',
      name: 'Morning health check',
      kind: 'request',
      connection_id: null,
      sql_text: null,
      saved_request_id: 'r4',
      request_method: null,
      request_url: null,
      request_headers_json: null,
      request_body: null,
      schedule_expr: '0 9 * * *',
      enabled: false,
      last_run: null,
      last_status: null,
      next_run: null,
      created_at: 0
    }
  ];

  const winOps = {
    'plugin:window|is_maximized': () => ok(false),
    'plugin:window|minimize': () => ok(null),
    'plugin:window|toggle_maximize': () => ok(null),
    'plugin:window|close': () => ok(null),
    'plugin:window|start_dragging': () => ok(null)
  };

  window.__TAURI_INTERNALS__ = {
    transformCallback: (cb) => cb,
    metadata: {
      currentWindow: { label: 'main' },
      currentWebview: { windowLabel: 'main', label: 'main' }
    },
    invoke: (cmd, args) => {
      if (winOps[cmd]) return winOps[cmd]();
      if (cmd.startsWith('plugin:')) return ok(null);
      return handlers[cmd]
        ? handlers[cmd](args || {})
        : Promise.reject(`mock: no handler for ${cmd}`);
    }
  };
  console.info('[OG TestDesk] mock IPC installed (not running in Tauri)');
}
