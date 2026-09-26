import Logging
import PostgresNIO

func remindOverdue(_ client: PostgresClient, logger: Logger) async throws {
    let rows = try await client.query("""
    select i.id, i.total, c.email
    from invoices i join customers c on c.id = i.customer_id
    where i.due_at < now() and i.paid_at is null
    order by i.due_at
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
