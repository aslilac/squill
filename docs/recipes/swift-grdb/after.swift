import GRDB

struct PlayerStore {
    let dbQueue: DatabaseQueue

    func migrate() throws {
        var migrator = DatabaseMigrator()
        migrator.registerMigration("v1") { db in
            try db.execute(sql: """
            create table player (
                id integer primary key autoincrement,
                name text not null,
                score integer not null default 0
            );
            create index player_on_score on player (score desc)
            """)
        }
        try migrator.migrate(dbQueue)
    }

    func leaders(minimum: Int) throws -> [Row] {
        try dbQueue.read { db in
            try Row.fetchAll(db, sql: """
            select name, score
            from player
            where score >= :minimum
            order by score desc
            limit 10
            """, arguments: ["minimum": minimum])
        }
    }

    func count() throws -> Int {
        try dbQueue.read { db in
            try Int.fetchOne(db, sql: "SELECT count(*) FROM player") ?? 0
        }
    }
}
