async fn overdue(client: &Client, cutoff: SystemTime) -> Result<Vec<Row>, Error> {
    client
        .query(
            r"
            select i.id, i.total, c.email
            from invoices i join customers c on c.id = i.customer_id
            where i.due_at < $1 and i.paid_at is null
            order by i.due_at
            ",
            &[&cutoff],
        )
        .await
}

async fn mark_paid(client: &Client, id: i64) -> Result<u64, Error> {
    client.execute("UPDATE invoices SET paid_at = now() WHERE id = $1", &[&id]).await
}
