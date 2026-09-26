async fn overdue(client: &Client, cutoff: SystemTime) -> Result<Vec<Row>, Error> {
    client
        .query(
            r"SELECT i.id, i.total, c.email FROM invoices i JOIN customers c ON c.id = i.customer_id WHERE i.due_at < $1 AND i.paid_at IS NULL ORDER BY i.due_at",
            &[&cutoff],
        )
        .await
}

async fn mark_paid(client: &Client, id: i64) -> Result<u64, Error> {
    client.execute("UPDATE invoices SET paid_at = now() WHERE id = $1", &[&id]).await
}
