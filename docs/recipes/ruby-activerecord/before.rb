class AddSearchToArticles < ActiveRecord::Migration[7.2]
  def up
    execute <<~SQL
      ALTER TABLE articles ADD COLUMN search tsvector GENERATED ALWAYS AS (to_tsvector('english', coalesce(title, '') || ' ' || coalesce(body, ''))) STORED;
      CREATE INDEX articles_search ON articles USING gin (search);
    SQL
  end
end

class Article < ApplicationRecord
  def self.search(term)
    find_by_sql([<<~SQL, term])
      SELECT id, title FROM articles WHERE search @@ plainto_tsquery('english', ?) ORDER BY published_at DESC
    SQL
  end

  def self.monthly_counts
    rows = connection.select_rows(<<~SQL)
      SELECT date_trunc('month', published_at) AS month, count(*) FROM articles WHERE published_at IS NOT NULL GROUP BY 1 ORDER BY 1
    SQL
    rows.to_h
  end

  def self.by_author(name)
    find_by_sql(<<~SQL)
      SELECT * FROM articles WHERE author = '#{name}'
    SQL
  end
end
