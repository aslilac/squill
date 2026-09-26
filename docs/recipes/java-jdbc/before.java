public List<Invoice> overdue(Connection conn, Instant cutoff) throws SQLException {
    try (var stmt = conn.prepareStatement(
            """
            SELECT i.id, i.total, c.email FROM invoices i JOIN customers c ON c.id = i.customer_id WHERE i.due_at < ? AND i.paid_at IS NULL ORDER BY i.due_at
            """)) {
        stmt.setTimestamp(1, Timestamp.from(cutoff));
        var invoices = new ArrayList<Invoice>();
        try (var rows = stmt.executeQuery()) {
            while (rows.next()) {
                invoices.add(Invoice.from(rows));
            }
        }
        return invoices;
    }
}
