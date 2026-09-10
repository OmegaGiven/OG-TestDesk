#!/usr/bin/env python3
"""Seed a demo SQLite DB + register connections / tabs / requests / env in
the OG TestDesk metadata store so there's something to click around in."""
import os, sqlite3, uuid, time, json, random, datetime

HERE = os.path.dirname(os.path.abspath(__file__))
DEMO_DB = os.path.join(HERE, "shop.db")
META_DB = os.path.expanduser("~/.local/share/OGTestDesk/og_testdesk.db")
now = int(time.time())

# ---------------------------------------------------------------- demo DB
if os.path.exists(DEMO_DB):
    os.remove(DEMO_DB)
d = sqlite3.connect(DEMO_DB)
d.executescript(
    """
    CREATE TABLE customers (
        id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT UNIQUE,
        city TEXT, created_at TEXT, lifetime_value REAL, is_active BOOLEAN
    );
    CREATE TABLE products (
        id INTEGER PRIMARY KEY, sku TEXT UNIQUE, name TEXT, category TEXT,
        price REAL, in_stock INTEGER, attributes TEXT
    );
    CREATE TABLE orders (
        id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id),
        status TEXT, placed_at TEXT, total REAL, notes TEXT
    );
    CREATE TABLE order_items (
        id INTEGER PRIMARY KEY, order_id INTEGER REFERENCES orders(id),
        product_id INTEGER REFERENCES products(id), qty INTEGER, unit_price REAL
    );
    CREATE VIEW customer_revenue AS
        SELECT c.id, c.name, c.city, COUNT(o.id) AS orders,
               ROUND(COALESCE(SUM(o.total),0), 2) AS revenue
        FROM customers c LEFT JOIN orders o ON o.customer_id = c.id
        GROUP BY c.id ORDER BY revenue DESC;
    """
)

cities = ["Dallas", "Austin", "Denver", "Portland", "Chicago", "Miami", "Seattle"]
first = ["Ava","Liam","Noah","Emma","Oliver","Mia","Ethan","Sophia","Lucas","Zoe","Mason","Isla"]
last = ["Ng","Patel","Reyes","Kim","Okafor","Silva","Larsen","Haddad","Cruz","Bauer"]
random.seed(7)

for i in range(1, 41):
    name = f"{random.choice(first)} {random.choice(last)}"
    created = datetime.date(2024, 1, 1) + datetime.timedelta(days=random.randint(0, 600))
    d.execute(
        "INSERT INTO customers VALUES (?,?,?,?,?,?,?)",
        (i, name, f"user{i}@example.com", random.choice(cities),
         created.isoformat(), round(random.uniform(0, 5000), 2), random.random() > 0.2),
    )

cats = ["Keyboards", "Mice", "Monitors", "Cables", "Audio"]
for i in range(1, 26):
    cat = random.choice(cats)
    d.execute(
        "INSERT INTO products VALUES (?,?,?,?,?,?,?)",
        (i, f"SKU-{1000+i}", f"{cat[:-1]} Model {i}", cat,
         round(random.uniform(9.99, 399.99), 2), random.randint(0, 250),
         json.dumps({"color": random.choice(["black","white","grey"]),
                     "warranty_months": random.choice([12, 24, 36])})),
    )

statuses = ["pending", "paid", "shipped", "delivered", "refunded"]
oid = 1
for cust in range(1, 41):
    for _ in range(random.randint(0, 4)):
        placed = datetime.datetime(2024, 6, 1) + datetime.timedelta(
            days=random.randint(0, 400), hours=random.randint(0, 23))
        items = random.randint(1, 4)
        total = 0.0
        rows = []
        for _ in range(items):
            pid = random.randint(1, 25)
            qty = random.randint(1, 3)
            price = d.execute("SELECT price FROM products WHERE id=?", (pid,)).fetchone()[0]
            total += price * qty
            rows.append((pid, qty, price))
        d.execute("INSERT INTO orders VALUES (?,?,?,?,?,?)",
                  (oid, cust, random.choice(statuses), placed.isoformat(),
                   round(total, 2), None if random.random() > 0.3 else "gift wrap"))
        for pid, qty, price in rows:
            d.execute("INSERT INTO order_items (order_id, product_id, qty, unit_price) VALUES (?,?,?,?)",
                      (oid, pid, qty, price))
        oid += 1

d.commit()
d.close()
print(f"demo DB: {DEMO_DB}  ({oid-1} orders)")

# ---------------------------------------------------------- metadata store
m = sqlite3.connect(META_DB)
m.execute("PRAGMA foreign_keys = ON")

sqlite_id = str(uuid.uuid4())
pg_id = str(uuid.uuid4())

m.execute("DELETE FROM connections WHERE nickname IN ('DEMO SHOP','LOCAL PG')")
m.execute(
    "INSERT INTO connections (id,nickname,kind,host,port,database,user,file_path,use_tls,color,sort_order,created_at) "
    "VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
    (sqlite_id, "DEMO SHOP", "sqlite", None, None, None, None, DEMO_DB, 0, "var(--conn-teal)", 0, now),
)
m.execute(
    "INSERT INTO connections (id,nickname,kind,host,port,database,user,file_path,use_tls,color,sort_order,created_at) "
    "VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
    (pg_id, "LOCAL PG", "postgres", "localhost", 5432, "postgres", "postgres", None, 0, "var(--conn-blue)", 1, now),
)

tabs = [
    ("Top customers", "SELECT * FROM customer_revenue LIMIT 15;"),
    ("Orders by status",
     "SELECT status, COUNT(*) AS n, ROUND(SUM(total),2) AS revenue\nFROM orders GROUP BY status ORDER BY revenue DESC;"),
    ("Order detail",
     "SELECT o.id, c.name, o.status, o.placed_at, oi.qty, p.name AS product, oi.unit_price\n"
     "FROM orders o\nJOIN customers c ON c.id = o.customer_id\n"
     "JOIN order_items oi ON oi.order_id = o.id\nJOIN products p ON p.id = oi.product_id\n"
     "ORDER BY o.id DESC LIMIT 100;"),
    ("By city + status",
     "-- fill {{city}} and {{status}} in the Variables bar above\n"
     "SELECT c.city, o.status, COUNT(*) AS orders, ROUND(SUM(o.total),2) AS revenue\n"
     "FROM orders o JOIN customers c ON c.id = o.customer_id\n"
     "WHERE c.city = '{{city}}' AND o.status = '{{status}}'\n"
     "GROUP BY c.city, o.status;"),
]
m.execute("DELETE FROM query_tabs WHERE connection_id = ?", (sqlite_id,))
for i, (title, sql) in enumerate(tabs):
    m.execute(
        "INSERT INTO query_tabs (id,connection_id,title,sql_text,position,is_active) VALUES (?,?,?,?,?,?)",
        (str(uuid.uuid4()), sqlite_id, title, sql, i, 1 if i == 0 else 0),
    )

# environment
m.execute("DELETE FROM environments WHERE name = 'Demo APIs'")
m.execute("UPDATE environments SET is_active = 0")
m.execute(
    "INSERT INTO environments (id,name,variables_json,is_active) VALUES (?,?,?,1)",
    (str(uuid.uuid4()), "Demo APIs",
     json.dumps({"baseUrl": "https://jsonplaceholder.typicode.com", "httpbin": "https://httpbin.org"})),
)

# request collection + saved requests
col_id = str(uuid.uuid4())
m.execute("DELETE FROM request_collections WHERE name = 'Playground'")
m.execute("INSERT INTO request_collections (id,name,parent_id) VALUES (?,?,NULL)", (col_id, "Playground"))
reqs = [
    ("List posts", "GET", "{{baseUrl}}/posts", {}, None),
    ("Get post 1", "GET", "{{baseUrl}}/posts/1", {}, None),
    ("Create post", "POST", "{{baseUrl}}/posts",
     {"Content-Type": "application/json"},
     json.dumps({"title": "hello", "body": "from OG TestDesk", "userId": 1}, indent=2)),
    ("Headers echo", "GET", "{{httpbin}}/headers", {"X-Demo": "true"}, None),
]
m.execute("DELETE FROM saved_requests WHERE collection_id = ?", (col_id,))
for i, (name, method, url, headers, body) in enumerate(reqs):
    m.execute(
        "INSERT INTO saved_requests (id,collection_id,name,method,url,headers_json,body,sort_order,created_at) "
        "VALUES (?,?,?,?,?,?,?,?,?)",
        (str(uuid.uuid4()), col_id, name, method, url, json.dumps(headers), body, i, now),
    )

m.commit()
m.close()
print("metadata seeded: 2 connections, 3 query tabs, 1 environment, 1 collection (4 requests)")
