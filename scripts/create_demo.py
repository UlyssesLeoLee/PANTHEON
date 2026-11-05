"""Create a sample SQLite database. Not for production datasets."""
import sqlite3
from pathlib import Path
path=Path(__file__).resolve().parent.parent / "data" / "pantheon-demo.db"
path.parent.mkdir(exist_ok=True)
if path.exists(): path.unlink()
with sqlite3.connect(path) as c:
 c.executescript('''PRAGMA foreign_keys=ON;
 CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT UNIQUE);
 CREATE TABLE projects (id INTEGER PRIMARY KEY, owner_id INTEGER NOT NULL REFERENCES users(id), title TEXT NOT NULL);
 CREATE TABLE tasks (id INTEGER PRIMARY KEY, project_id INTEGER NOT NULL REFERENCES projects(id), status TEXT);
 INSERT INTO users VALUES (1,'Ada Lovelace','ada@example.com'),(2,'Grace Hopper','grace@example.com');
 INSERT INTO projects VALUES (1,1,'PANTHEON'),(2,2,'Database Design');
 INSERT INTO tasks VALUES (1,1,'in_progress'),(2,1,'done'),(3,2,'todo');''')
print(path)
