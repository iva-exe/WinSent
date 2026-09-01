// Stav ochrany Windows jako řádky pro vykreslení.
//
// Bydlí to tady, a ne v sekci Security, protože totéž potřebuje
// i dlaždice na Home. Dvě kopie týchž pravidel by se rozešly hned
// u prvního doplnění — a rozdíl mezi „4 v pořádku" na přehledu
// a jiným počtem v sekci by nešel vysvětlit ničím jiným než chybou.
import {
	FileLock2,
	Flame,
	HardDrive,
	RefreshCw,
	Shield,
	ShieldCheck,
	ShieldAlert,
	ShieldOff,
	UserCheck
} from 'lucide-svelte';

function fmtDay(t) {
	return new Date(t * 1000).toLocaleDateString('cs-CZ');
}

/// Řádky: { icon, name, state, detail, tone } — tón je 'ok' | 'warn' | 'dim'.
/// 'dim' znamená „nezjištěno nebo se to sem nehodí", ne „v pořádku".
export function ochranaRadky(report) {
		if (!report) return [];
		const p = report.protection;
		const rows = [];

		// Antivirus.
		const av = p.av.filter(([n]) => n);
		// Osiřelé registrace se nepočítají za ochranu. Security Center
		// po odinstalaci registraci nemusí uklidit, takže tam roky visí
		// antivirus, který na počítači není — a tvrdí, že běží.
		const live = av.filter(([, , , leftover]) => !leftover);
		const gone = av.filter(([, , , leftover]) => leftover);
		if (live.length) {
			for (const [name, enabled, fresh] of live) {
				const d = p.defender;
				const extra =
					d && name.toLowerCase().includes('defender')
						? d[1] != null
							? ` · definice ${d[1] === 0 ? 'dnešní' : `staré ${d[1]} d`}` +
								(d[2] != null ? ` · rychlý sken před ${d[2]} d` : '')
							: ''
						: '';
				rows.push({
					icon: ShieldCheck,
					name,
					state: enabled ? 'běží' : 'vypnutý',
					detail: (enabled ? (fresh ? 'aktuální definice' : 'ZASTARALÉ definice') : '') + extra,
					tone: enabled && fresh ? 'ok' : 'warn'
				});
			}
		} else {
			rows.push({
				icon: ShieldCheck,
				name: 'Antivirus',
				state: 'nenalezen',
				detail: 'Security Center žádný nehlásí',
				tone: 'warn'
			});
		}
		// Dva současně běžící antiviry si navzájem berou soubory pod
		// rukama — je to informace, ne příkaz: který z nich odejde, je
		// rozhodnutí uživatele.
		const active = live.filter(([, enabled]) => enabled).map(([n]) => n);
		if (active.length > 1) {
			rows.push({
				icon: ShieldAlert,
				name: 'Dva antiviry naráz',
				state: `${active.length} současně`,
				detail: `${active.join(' + ')} — oba hlídají soubory v reálném čase`,
				tone: 'warn'
			});
		}
		// Zbytek po odinstalovaném antiviru: ne poplach, ale vysvětlení,
		// proč ho Windows možná pořád někde uvádějí.
		for (const [name] of gone) {
			rows.push({
				icon: ShieldOff,
				name,
				state: 'zbytek po odinstalaci',
				detail: 'registrace ve Windows zůstala, program na disku není — nechrání nic',
				tone: 'dim'
			});
		}

		// Firewall — tři profily.
		const fw = [
			['doména', p.fw_domain],
			['privátní', p.fw_private],
			['veřejná', p.fw_public]
		];
		const fwOff = fw.filter(([, v]) => v === false).map(([n]) => n);
		rows.push({
			icon: Flame,
			name: 'Firewall',
			state: fwOff.length === 0 ? 'zapnutý' : `vypnutý (${fwOff.join(', ')})`,
			detail: fw.map(([n, v]) => `${n}: ${v == null ? '—' : v ? 'ano' : 'NE'}`).join(' · '),
			tone: fwOff.length === 0 ? 'ok' : 'warn'
		});

		// Secure Boot: None = legacy BIOS, kde neexistuje.
		rows.push({
			icon: Shield,
			name: 'Secure Boot',
			state: p.secure_boot == null ? 'není k dispozici' : p.secure_boot ? 'zapnutý' : 'vypnutý',
			detail:
				p.secure_boot == null
					? 'stroj bootuje přes legacy BIOS — Secure Boot na něm neexistuje'
					: '',
			tone: p.secure_boot == null ? 'dim' : p.secure_boot ? 'ok' : 'warn'
		});

		// TPM. Prázdná verze specifikace = čip ve stromu zařízení je,
		// ale WMI o něm mlčí (typicky bez práv). „Nenalezen" by v té
		// chvíli bylo tvrzení o hardwaru, které neplyne z ničeho.
		rows.push({
			icon: FileLock2,
			name: 'TPM',
			state: p.tpm == null ? 'nenalezen' : p.tpm[1] ? (p.tpm[0] ? 'zapnutý' : 'vypnutý') : 'nezjištěno',
			detail: p.tpm
				? p.tpm[1]
					? `specifikace ${p.tpm[1]}`
					: 'čip v počítači je, ale Windows o něm přes WMI nic neřekly'
				: 've stromu zařízení žádný čip není',
			tone: p.tpm?.[0] && p.tpm?.[1] ? 'ok' : 'dim'
		});

		// UAC.
		rows.push({
			icon: UserCheck,
			name: 'Řízení uživatelských účtů (UAC)',
			state: p.uac_enabled ? 'zapnuté' : 'VYPNUTÉ',
			detail: p.uac_enabled
				? p.uac_admin_prompt === 0
					? 'výzvy jsou potlačené — správce se nikdy neptá'
					: 'výzva při změnách vyžadujících správce'
				: 'programy mohou měnit systém bez ptaní',
			tone: p.uac_enabled && p.uac_admin_prompt !== 0 ? 'ok' : 'warn'
		});

		// Šifrování disků.
		if (p.encryption.length) {
			const on = p.encryption.filter(([, s]) => s === 1).map(([l]) => l);
			const off = p.encryption.filter(([, s]) => s === 0).map(([l]) => l);
			rows.push({
				icon: HardDrive,
				name: 'Šifrování disku (BitLocker)',
				state: on.length ? `zapnuté (${on.join(', ')})` : 'vypnuté',
				detail: off.length ? `nešifrované svazky: ${off.join(', ')}` : '',
				// Nešifrovaný disk je fakt, ne poplach — spousta strojů
				// BitLocker vědomě nepoužívá.
				tone: on.length ? 'ok' : 'dim'
			});
		}

		// Stav aktualizací. Neinstaluje se odsud nic — je to informace
		// o tom, jestli si systém záplaty vůbec bere. Čas poslední
		// instalace novější Windows v registru nevedou, a tak se místo
		// vymyšleného data přizná, že se nezjistil.
		const o = p.os;
		if (o?.build) {
			const svcOff = o.update_service_start === 4 || o.update_disabled_by_policy;
			const days =
				o.update_last_search != null
					? Math.floor((Date.now() / 1000 - o.update_last_search) / 86400)
					: null;
			rows.push({
				icon: RefreshCw,
				name: 'Aktualizace Windows',
				state: svcOff
					? 'VYPNUTÉ'
					: days == null
						? 'nezjištěno'
						: days === 0
							? 'kontrolováno dnes'
							: `kontrolováno před ${days} dny`,
				detail: [
					o.update_disabled_by_policy ? 'zakázané zásadou' : null,
					o.update_service_start === 4 ? 'služba zakázaná' : null,
					o.update_last_install ? `instalováno ${fmtDay(o.update_last_install)}` : null
				]
					.filter(Boolean)
					.join(' · '),
				tone: svcOff ? 'warn' : days != null && days > 30 ? 'warn' : days == null ? 'dim' : 'ok'
			});
		}
		return rows;
}
