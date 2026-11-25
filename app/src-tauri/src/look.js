// Runs in a page Rigbrew looks at for a Code task, before the page's own scripts (see look.rs).
// It keeps what goes wrong while the page runs (errors on the console, errors nothing caught,
// requests that failed), can click and type for a simple check, and says what the page shows.
(function () {
	if (window.top !== window || window.__rigbrewLook) return;
	var MAX = 30;
	var errors = [];
	var failed = [];
	var notes = [];
	var acting = false;

	function add(list, text) {
		try {
			text = String(text);
		} catch (e) {
			text = '?';
		}
		if (list.length < MAX) list.push(text.slice(0, 500));
	}

	function show(value) {
		if (value instanceof Error) return (value.name || 'Error') + ': ' + value.message;
		if (typeof value === 'string') return value;
		try {
			return JSON.stringify(value);
		} catch (e) {
			return String(value);
		}
	}

	var consoleError = console.error;
	console.error = function () {
		add(errors, Array.prototype.map.call(arguments, show).join(' '));
		return consoleError.apply(console, arguments);
	};

	window.addEventListener(
		'error',
		function (event) {
			var from = event.target;
			// A picture, script or stylesheet that did not load.
			if (from && from !== window && (from.src || from.href)) {
				add(failed, 'GET ' + (from.src || from.href) + ' (did not load)');
				return;
			}
			var where = event.filename ? ' (' + event.filename + ':' + event.lineno + ':' + event.colno + ')' : '';
			add(errors, (event.message || 'Error') + where);
		},
		true
	);
	window.addEventListener('unhandledrejection', function (event) {
		add(errors, 'Unhandled promise rejection: ' + show(event.reason));
	});

	var realFetch = window.fetch;
	if (realFetch) {
		window.fetch = function (input, init) {
			var method = String((init && init.method) || (input && input.method) || 'GET').toUpperCase();
			var url = typeof input === 'string' ? input : (input && input.url) || String(input);
			return realFetch.apply(this, arguments).then(
				function (answer) {
					if (!answer.ok) add(failed, method + ' ' + url + ' ' + answer.status);
					return answer;
				},
				function (why) {
					add(failed, method + ' ' + url + ' (' + show(why) + ')');
					throw why;
				}
			);
		};
	}
	var open = XMLHttpRequest.prototype.open;
	var send = XMLHttpRequest.prototype.send;
	XMLHttpRequest.prototype.open = function (method, url) {
		this.__rigbrewAsked = String(method).toUpperCase() + ' ' + url;
		return open.apply(this, arguments);
	};
	XMLHttpRequest.prototype.send = function () {
		var request = this;
		request.addEventListener('loadend', function () {
			if (request.status === 0 || request.status >= 400) add(failed, request.__rigbrewAsked + ' ' + (request.status || '(no answer)'));
		});
		return send.apply(this, arguments);
	};

	// A dialog would hold a window nobody sees for ever.
	window.alert = function (message) {
		add(notes, 'The page showed an alert: ' + message);
	};
	window.confirm = function (message) {
		add(notes, 'The page asked to confirm (answered yes): ' + message);
		return true;
	};
	window.prompt = function (message) {
		add(notes, 'The page asked for input (none given): ' + message);
		return null;
	};

	function words(element) {
		var said = element.innerText || element.value || element.getAttribute('aria-label') || element.getAttribute('placeholder') || element.getAttribute('name') || '';
		return String(said).replace(/\s+/g, ' ').trim().toLowerCase();
	}

	/** An element by CSS selector, or by what it says. */
	function find(target, typing) {
		try {
			var bySelector = document.querySelector(target);
			if (bySelector) return bySelector;
		} catch (e) {
			// not a selector: look for the words
		}
		var wanted = String(target).replace(/\s+/g, ' ').trim().toLowerCase();
		var kinds = typing
			? 'input, textarea, select, [contenteditable=""], [contenteditable="true"]'
			: 'button, a, [role="button"], input[type="submit"], input[type="button"], summary, label, [onclick], [role="tab"], [role="link"]';
		var all = Array.prototype.slice.call(document.querySelectorAll(kinds));
		var i;
		if (typing) {
			// A field is often named by the label next to it.
			var labels = document.querySelectorAll('label');
			for (i = 0; i < labels.length; i++) {
				if (words(labels[i]).indexOf(wanted) !== -1 && labels[i].control) return labels[i].control;
			}
		}
		for (i = 0; i < all.length; i++) if (words(all[i]) === wanted) return all[i];
		for (i = 0; i < all.length; i++) if (words(all[i]).indexOf(wanted) !== -1) return all[i];
		return null;
	}

	function type(element, text) {
		element.focus();
		if (element.isContentEditable) {
			element.textContent = text;
		} else {
			// Through the element's own setter, so a framework that watches the field notices.
			var kind = element.tagName === 'TEXTAREA' ? HTMLTextAreaElement : element.tagName === 'SELECT' ? HTMLSelectElement : HTMLInputElement;
			var setter = Object.getOwnPropertyDescriptor(kind.prototype, 'value');
			if (setter && setter.set) setter.set.call(element, text);
			else element.value = text;
		}
		element.dispatchEvent(new Event('input', { bubbles: true }));
		element.dispatchEvent(new Event('change', { bubbles: true }));
	}

	/**
	 * Clicks and types, one step after the other with a moment in between. What a step needs
	 * may not be on the page yet (it is still loading, or the step before brings it): it is
	 * looked for again for a few seconds before the step is given up.
	 */
	function act(steps) {
		var at = 0;
		var since = Date.now();
		acting = true;
		function next() {
			if (at >= steps.length) {
				acting = false;
				return;
			}
			var step = steps[at];
			var target = step.click || step.type;
			try {
				var element = find(target, !step.click);
				if (!element && Date.now() - since < 3000) {
					setTimeout(next, 150);
					return;
				}
				if (!element) {
					add(notes, 'Nothing on the page matches "' + target + '", so that step was skipped.');
				} else if (step.click) {
					if (element.scrollIntoView) element.scrollIntoView({ block: 'center' });
					element.click();
				} else {
					type(element, String(step.text || ''));
				}
			} catch (e) {
				add(notes, 'A step did not work: ' + show(e));
			}
			at++;
			since = Date.now();
			setTimeout(next, 400);
		}
		next();
		return steps.length;
	}

	function report() {
		return {
			url: location.href,
			title: document.title || '',
			text: ((document.body && document.body.innerText) || '').slice(0, 6000),
			errors: errors,
			failed: failed,
			note: notes.join(' '),
			busy: acting
		};
	}

	Object.defineProperty(window, '__rigbrewLook', { value: { act: act, report: report }, enumerable: false });
})();
