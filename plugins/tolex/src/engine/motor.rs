//! Motor — the lumped electro-mechanical driver core, a LEAF in the
//! NonlinearRc/triode tradition: a dense cross-coupled cell, f64
//! states, implicitly integrated. Three states — coil current `i`,
//! cone velocity `v`, displacement `x` — under the classic mesh:
//!
//! ```text
//!  Le·di/dt = e − (Re + Rsrc)·i − Bl(x)·v
//! Mms·dv/dt = Bl(x)·i − Rms·v − [k_m·(1+(x/xk)²) + k_box]·x
//!    dx/dt = v
//! ```
//!
//! Everything derives from four knobs and a size:
//! Cms = 1/(Mms·ω₀²), Rms = ω₀·Mms/Qms, Bl = √(ω₀·Mms·Re/Qes) with
//! 1/Qts = 1/Qes + 1/Qms — consistent by construction, no free
//! constants to drift. The large-signal terms are the Klippel
//! tradition's first pair: Bl(x) droops (flux leaves the gap),
//! Cms(x) stiffens (the spider runs out of throw) — together the
//! excursion compression + low H2/H3 that a frozen IR cannot render.
//!
//! Discretization: trapezoidal, semi-implicit — Bl and the secant
//! stiffness are frozen at the step's start state, then the LINEAR
//! 3×3 step solves exactly (A-stable at any stiffness, so the
//! hardening spring cannot blow the step). Output is on-axis
//! far-field pressure ∝ cone ACCELERATION, taken from the solved
//! step's own derivative, normalized so the mass-controlled band
//! sits at unity for the linear small-signal motor — which makes
//! `drive` buy excursion, never volume.

/// Per-size physical scale: cone area Sd (m²) and moving mass Mms
/// (kg), guitar-driver-typical. The params-side menu (SIZES) is the
/// UI twin of this table.
const SIZES: [(f64, f64); 4] = [
    (0.022, 0.020), // 8"
    (0.035, 0.028), // 10"
    (0.055, 0.035), // 12"
    (0.088, 0.060), // 15"
];

/// Voice-coil DC resistance (8 Ω nominal driver).
const RE: f64 = 6.4;
/// EFFECTIVE series inductance. Real coils are semi-inductive
/// (≈K·s^0.6); until the LR2 ladder lands, a reduced pure Le
/// keeps the electrical corner honest at solid-state damping.
const LE: f64 = 0.25e-3;
/// House mechanical Q — guitar suspensions sit near 8; the q knob
/// (Qts) apportions the rest to the electrical side.
const QMS: f64 = 8.0;
const RHO: f64 = 1.204;
const C_AIR: f64 = 343.0;
/// Bl(x) droop scale. GUITAR-SPEAKER numbers, not woofer numbers:
/// a G12-class coil is deliberately short (Xmax ~0.5-1 mm) — the
/// speaker is BUILT to run into its motor. −50% flux at 1.2 mm.
/// (First cut used 4 mm — a hi-fi scale that made the living leg
/// unreachable from program material; field-reported.)
const XB2: f64 = 1.2e-3 * 1.2e-3;
/// Cms(x) hardening scale: stiffness doubled at 1 mm.
const XK2: f64 = 1.0e-3 * 1.0e-3;
/// The hardening/droop scale ratio — how much the suspension
/// stiffens per unit of the (XB2-normalized) excursion envelope.
pub(crate) const HARDEN_PER_ENV: f64 = XB2 / XK2;
/// Mechanical stop — a state guard for parameter-sweep extremes,
/// not a musical device (the hardening spring does the musical work).
const X_STOP: f64 = 0.005;
/// ASYMMETRY — the even-order voice (real motors speak H2). The
/// coil sits slightly OFFSET in the gap and the spider is stiffer
/// one way than the other — both are linear tilts on the
/// large-signal laws, scale ~1/(2·Xmax-class).
const BL_TILT_PER_M: f64 = 11.0;
const K_TILT_PER_M: f64 = 18.0;

/// THE HEAT: the coil dissipates i²·Re into copper that can
/// only shed ~R_TH kelvin per watt; copper's resistivity rises
/// 0.39%/K, the hot coil steals drive, the tone sags and leans on
/// the mids. Seconds-scale — the "speakers warming up" feel no IR
/// can render. (Magnet/former slow pole joins later; one honest
/// time constant first.)
const R_TH_K_PER_W: f64 = 2.5;
const TEMPCO_PER_K: f64 = 0.0039;
const TAU_COIL_S: f64 = 2.0;

/// FLUX MODULATION: the coil's
/// inductance rides its POSITION in the gap (moving out sheds
/// inductance — asymmetric, the mid-band's H2 source) and SATURATES
/// with current (iron only carries so much flux). Both frozen per
/// step like Bl(x).
const LE_TILT_PER_M: f64 = 80.0;
const I_SAT_A: f64 = 4.0;
/// Bl(i): the coil's own flux aids or opposes the magnet — the
/// mid-band's strongest even-order source (current is large at
/// mids even when travel is nil).
const I_FLUX_A: f64 = 260.0;

/// Force reference for coil/cone STRESS: ~the motor force at a
/// 25 W-class rating (Bl ~11 T·m × ~1.8 A). Displacement collapses
/// as 1/f² — mid-heavy program never excurses a cone — but a hard-
/// driven coil still stresses the cone through FORCE, and that is
/// what softens breakup on real speakers fed mids at volume.
const F_REF: f64 = 20.0;

/// The damping knob's physical map — shared by the engine bind and
/// the display twin. Square law: the middle feels like a healthy
/// feedback amp, the top opens into the no-NFB bloom.
pub(crate) fn rsrc_ohms(damping: f32) -> f32 {
    40.0 * damping * damping
}

/// The derived constant sheet — one derivation for the audio path
/// (set()) and the display twin (linear_response_db).
pub(crate) struct MotorConsts {
    pub le: f64,
    pub rt: f64,
    pub bl: f64,
    pub mms: f64,
    pub k: f64,
    /// The box's share of k — it does NOT harden with excursion
    /// (only the suspension does); operating-point displays split
    /// on this.
    pub k_box: f64,
    pub rms: f64,
    pub cal: f64,
}

pub(crate) fn derive_consts(
    size: usize,
    fs: f32,
    qts: f32,
    rsrc: f32,
    vb_m3: Option<f32>,
    mass_scale: f32,
) -> MotorConsts {
    let (sd, mms) = SIZES[size.min(SIZES.len() - 1)];
    let mms = mms * f64::from(mass_scale.clamp(0.25, 4.0));
    let fs = f64::from(fs.max(20.0));
    let w0 = core::f64::consts::TAU * fs;
    let qts = f64::from(qts).clamp(0.1, QMS * 0.9);
    let qes = qts * QMS / (QMS - qts);
    let k_m = mms * w0 * w0;
    let rms = w0 * mms / QMS;
    let bl = (w0 * mms * RE / qes).sqrt();
    let rt = RE + f64::from(rsrc.max(0.0));
    let k_box = match vb_m3 {
        Some(vb) => RHO * C_AIR * C_AIR * sd * sd / f64::from(vb.max(1e-3)),
        None => 0.0,
    };
    MotorConsts {
        le: LE,
        rt,
        bl,
        mms,
        k: k_m + k_box,
        k_box,
        rms,
        cal: mms * rt / bl,
    }
}

/// Small-signal |H| in dB — the display twin of the mesh at x ≈ 0
/// (Bl = Bl₀, k = k_m + k_box): a = jωV, V = Bl·I/Zm,
/// I = E/(Ze + Bl²/Zm), normalized by cal like tick().
pub(crate) fn linear_response_db(c: &MotorConsts, hz: f32) -> f32 {
    let w = core::f64::consts::TAU * f64::from(hz.max(1.0));
    // Zm = Rms + j(ωM − k/ω); Ze = Rt + jωLe
    let (zm_r, zm_i) = (c.rms, w * c.mms - c.k / w);
    let zm2 = zm_r * zm_r + zm_i * zm_i;
    // Bl²/Zm = Bl²·conj(Zm)/|Zm|²
    let (r_r, r_i) = (c.bl * c.bl * zm_r / zm2, -c.bl * c.bl * zm_i / zm2);
    let (zt_r, zt_i) = (c.rt + r_r, w * c.le + r_i);
    // a = jω·Bl / (Zm·Zt)
    let (d_r, d_i) = (zm_r * zt_r - zm_i * zt_i, zm_r * zt_i + zm_i * zt_r);
    let d2 = (d_r * d_r + d_i * d_i).max(1e-30);
    let a_mag = w * c.bl / d2.sqrt();
    (20.0 * (a_mag * c.cal).max(1e-15).log10()) as f32
}

pub struct Motor {
    // Derived constants (set()):
    le: f64,
    rt: f64,
    bl0: f64,
    mms: f64,
    k_m: f64,
    rms: f64,
    k_box: f64,
    cal: f64,
    // Rate contract:
    dt: f64,
    rate: f64,
    // States:
    i: f64,
    v: f64,
    x: f64,
    /// Excursion envelope (|x| / the Bl-droop scale, ~10 ms lag).
    env: f64,
    /// Force envelope ((Bl·i / F_REF)², same lag) — the coil-stress
    /// half of the working state.
    env_f: f64,
    env_k: f64,
    /// Coil temperature rise (K) and its rate constant. The solve
    /// sees rt + the thermal delta; cal keeps the COLD normalization
    /// so the sag is heard.
    t_coil: f64,
    th_k: f64,
}

impl Motor {
    pub fn new() -> Self {
        Self {
            le: LE,
            rt: RE,
            bl0: 1.0,
            mms: 1.0,
            k_m: 1.0,
            rms: 1.0,
            k_box: 0.0,
            cal: 1.0,
            dt: 0.0,
            rate: 0.0,
            i: 0.0,
            v: 0.0,
            x: 0.0,
            env: 0.0,
            env_f: 0.0,
            env_k: 0.0,
            t_coil: 0.0,
            th_k: 0.0,
        }
    }

    pub fn prepare(&mut self, rate: f32) {
        self.rate = f64::from(rate.max(1.0));
        self.dt = 1.0 / self.rate;
        self.env_k = 1.0 - (-1.0 / (0.010 * self.rate)).exp();
        self.th_k = 1.0 - (-1.0 / (TAU_COIL_S * self.rate)).exp();
        self.reset();
    }

    pub fn reset(&mut self) {
        self.i = 0.0;
        self.v = 0.0;
        self.x = 0.0;
        self.env = 0.0;
        self.env_f = 0.0;
        self.t_coil = 0.0;
    }

    /// Excursion envelope, ~[0, 1+]: 1 means the cone is living at
    /// the Bl-droop scale (displacement — the travel strip's truth).
    #[inline]
    pub fn excursion(&self) -> f32 {
        self.env as f32
    }

    /// The WORKING state: displacement stress + coil-force stress.
    /// Mid-heavy program cannot excurse a cone (1/f²) but a driven
    /// coil still works it — breakup softening and the live display
    /// key on this.
    #[inline]
    pub fn work(&self) -> f32 {
        (self.env + 0.8 * self.env_f) as f32
    }

    /// Re-voice IN PLACE — block-rate safe, and the cone's position
    /// history is KEPT (a knob move must not teleport the cone).
    /// `vb_m3` is the sealed volume behind the cone; `None` = open
    /// back, no air spring.
    pub fn set(
        &mut self,
        size: usize,
        fs: f32,
        qts: f32,
        rsrc: f32,
        vb_m3: Option<f32>,
        mass_scale: f32,
    ) {
        // One derivation for audio and display (derive_consts). The
        // box spring is folded back out of k for the audio path's
        // nonlinear split (k_m hardens with x, k_box does not).
        let sheet = derive_consts(size, fs, qts, rsrc, vb_m3, mass_scale);
        self.mms = sheet.mms;
        self.rms = sheet.rms;
        self.bl0 = sheet.bl;
        self.rt = sheet.rt;
        self.k_box = sheet.k_box;
        self.k_m = sheet.k - sheet.k_box;
        // Mass-band unity: a ≈ Bl·i/Mms with i ≈ e/Rt, so
        // a·(Mms·Rt)/(Bl₀·e) ≈ 1 — the same Rt in the numerator keeps
        // the damping knob from doubling as a volume knob.
        self.cal = sheet.cal;
    }

    /// One step: `x_in` (±1 program) swings `v_drive` volts at the
    /// mesh; the return is normalized far-field pressure.
    #[inline]
    pub fn tick(&mut self, x_in: f32, v_drive: f32) -> f32 {
        let e = f64::from(x_in) * f64::from(v_drive);
        let (i0, v0, x0) = (self.i, self.v, self.x);
        let h = 0.5 * self.dt;
        // The hot coil: yesterday's watts in today's resistance.
        let rt = self.rt + RE * TEMPCO_PER_K * self.t_coil;
        // The live coil: inductance rides position and current.
        let le = self.le * (1.0 - LE_TILT_PER_M * x0).clamp(0.6, 1.4)
            / (1.0 + 0.15 * (i0 / I_SAT_A) * (i0 / I_SAT_A));

        // Large-signal coefficients, frozen over the step. The even
        // terms (droop, hardening) compress; the odd TILTS are the
        // asymmetry that gives the motor its H2.
        let bl = self.bl0 / (1.0 + x0 * x0 / XB2)
            * ((1.0 - BL_TILT_PER_M * x0) * (1.0 - i0 / I_FLUX_A)).clamp(0.5, 1.5);
        let k = self.k_m * ((1.0 + x0 * x0 / XK2) * (1.0 + K_TILT_PER_M * x0)).clamp(0.4, 8.0)
            + self.k_box;

        // (I − hA)·s₁ = (I + hA)·s₀ + dt·B·e, eliminated by hand:
        let m11 = 1.0 + h * rt / le;
        let m12 = h * bl / le;
        let m21 = -h * bl / self.mms;
        let m22 = 1.0 + h * self.rms / self.mms;
        let m23 = h * k / self.mms;

        let r1 = i0 + h * (-rt * i0 - bl * v0) / le + self.dt * e / le;
        let r2 = v0 + h * (bl * i0 - self.rms * v0 - k * x0) / self.mms;
        let r3 = x0 + h * v0;

        // Row 3 (x₁ = r3 + h·v₁) folded into row 2:
        let b2 = m22 + h * m23;
        let c2 = r2 - m23 * r3;
        let det = m11 * b2 - m12 * m21;
        let i1 = (r1 * b2 - m12 * c2) / det;
        let v1 = (m11 * c2 - r1 * m21) / det;
        let x1 = (r3 + h * v1).clamp(-X_STOP, X_STOP);

        self.i = i1;
        self.v = v1;
        self.x = x1;
        let stroke = (x1 * x1) / XB2;
        self.env += self.env_k * (stroke - self.env);
        let f_norm = bl * i1 / F_REF;
        self.env_f += self.env_k * (f_norm * f_norm - self.env_f);
        self.t_coil += self.th_k * (i1 * i1 * RE * R_TH_K_PER_W - self.t_coil);

        let accel = (bl * i1 - self.rms * v1 - k * x1) / self.mms;
        (accel * self.cal / f64::from(v_drive.max(1e-6))) as f32
    }
}
