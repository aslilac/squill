import GRDB

struct PlayerStore {
    let dbQueue: DatabaseQueue

    func migrate() throws {
        var migrator = DatabaseMigrator()
        migrator.registerMigration("v1") { db in
            try db.execute(sql: """
                CREATE TABLE player (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, score INTEGER NOT NULL DEFAULT 0);
                CREATE INDEX player_on_score ON player(score DESC);
                """)
        }
        try migrator.migrate(dbQueue)
    }

    func leaders(minimum: Int) throws -> [Row] {
        try dbQueue.read { db in
            try Row.fetchAll(db, sql: """
                SELECT name, score FROM player WHERE score >= :minimum ORDER BY score DESC LIMIT 10
                """, arguments: ["minimum": minimum])
        }
    }

    func count() throws -> Int {
        try dbQueue.read { db in
            try Int.fetchOne(db, sql: "SELECT count(*) FROM player") ?? 0
        }
    }
}
