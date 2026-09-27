import Logging
import PostgresNIO

func remindOverdue(_ client: PostgresClient, logger: Logger) async throws {
    let rows = try await client.query("""
        SELECT i.id, i.total, c.email FROM invoices i
        JOIN customers c ON c.id = i.customer_id WHERE i.due_at < now()
        AND i.paid_at IS NULL ORDER BY i.due_at
        """, logger: logger)
    for try await (id, total, email) in rows.decode((Int, Decimal, String).self) {
        logger.info("\(email) owes \(total) on invoice \(id)")
    }
}

func markPaid(_ client: PostgresClient, id: Int, logger: Logger) async throws {
    try await client.query("""
        UPDATE invoices SET paid_at = now() WHERE id = \(id)
        """, logger: logger)
}
