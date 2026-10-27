// Create the e2e database when it is not there yet. The API migrates and seeds it on boot,
// but cannot create it: it connects to it first.
//
// The database is the suite's own and holds nothing worth keeping, so one this checkout
// cannot boot against starts over: a migration from another branch the API does not know,
// or one whose file has changed since it ran. Either would stop the API at startup.
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';

import pg from 'pg';

const url = new URL(process.env.DATABASE_URL);
const name = url.pathname.slice(1);
const quoted = `"${name.replaceAll('"', '""')}"`;

/** The migrations in this checkout, by version, as sqlx checksums them (SHA-384). */
function checkout() {
	const dir = new URL('../../migrations/', import.meta.url);
	const migrations = new Map();
	for (const file of readdirSync(dir)) {
		const match = /^(\d+)_.*\.sql$/.exec(file);
		if (!match) continue;
		const sql = readFileSync(new URL(file, dir));
		migrations.set(BigInt(match[1]), createHash('sha384').update(sql).digest('hex'));
	}
	return migrations;
}

/** Whether the database ran a migration this checkout does not have, or a different one. */
async function divergent() {
	const db = new pg.Client({ connectionString: url.toString() });
	await db.connect();
	try {
		const { rows } = await db.query(
			`select version, encode(checksum, 'hex') as checksum from _sqlx_migrations`,
		);
		const local = checkout();
		return rows.some((row) => local.get(BigInt(row.version)) !== row.checksum);
	} catch {
		// No migrations table yet: nothing has run, so nothing can disagree.
		return false;
	} finally {
		await db.end();
	}
}

const admin = new URL(url);
admin.pathname = '/postgres';
const client = new pg.Client({ connectionString: admin.toString() });
await client.connect();
try {
	const { rowCount: found } = await client.query('select 1 from pg_database where datname = $1', [
		name,
	]);
	if (found > 0 && (await divergent())) {
		console.log(`${name} was migrated by another checkout; starting it over`);
		await client.query(`drop database ${quoted} with (force)`);
	}
	const { rowCount: left } = await client.query('select 1 from pg_database where datname = $1', [
		name,
	]);
	if (left === 0) await client.query(`create database ${quoted}`);
} finally {
	await client.end();
}
