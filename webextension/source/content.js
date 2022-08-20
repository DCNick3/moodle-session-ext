import optionsStorage from './options-storage.js';
import browser from "webextension-polyfill";

console.log('💈 Content script loaded for', chrome.runtime.getManifest().name);
async function init() {
	browser.runtime.sendMessage({
		"type": "extend_session",
	})
}

init();
