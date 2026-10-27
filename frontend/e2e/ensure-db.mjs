// Create the e2e database when it is not there yet. The API migrates and seeds it on boot,
// but cannot create it: it connects to it first.
import pg from 'pg';

const url = new URL(process.env.DATABASE_URL);
const name = url.pathname.slice(1);
url.pathname = '/postgres';
const client = new pg.Client({ connectionString: url.toString() });
await client.connect();
try {
	const { rowCount } = await client.query('select 1 from pg_database where datname = $1', [name]);
	if (rowCount === 0) await client.query(`create database "${name.replaceAll('"', '""')}"`);
} finally {
	await client.end();
}
