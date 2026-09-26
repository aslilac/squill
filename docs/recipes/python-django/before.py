from django.db import connection

from .models import Article


def popular(days=30):
    return Article.objects.raw(
        """SELECT a.id, a.title, count(v.id) AS views FROM blog_article a JOIN blog_view v ON v.article_id = a.id WHERE v.viewed_at > now() - make_interval(days => %s) GROUP BY a.id ORDER BY views DESC""",
        [days],
    )


def purge_drafts(author_id, cutoff):
    with connection.cursor() as cursor:
        cursor.execute(
            """DELETE FROM blog_article WHERE author_id = %s AND status = 'draft' AND updated_at < %s""",
            [author_id, cutoff],
        )
        return cursor.rowcount
