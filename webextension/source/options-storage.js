import OptionsSync from 'webext-options-sync';

export default new OptionsSync({
	defaults: {
		server_url: 'https://moodle-session-ext.dcnick3.me',
	},
	migrations: [
		OptionsSync.migrations.removeUnused,
	],
	logging: true,
});
