// Initialize Leaflet Map
// Centered roughly on Brazil / Congonhas area
const map = L.map('map', {
    zoomControl: false // Disable default zoom to position it elsewhere or keep clean
}).setView([-23.6, -46.6], 9);

// Add standard OpenStreetMap tiles
// The CSS will invert the colors to make it look like a dark radar map!
L.tileLayer('https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png', {
    maxZoom: 19,
    attribution: '© OpenStreetMap'
}).addTo(map);

// Dictionary to keep track of markers on the map
const markers = {};
const flightPaths = {};
let selectedIcao = null;

// DOM Elements
const statAirborne = document.getElementById('stat-airborne');
const statGround = document.getElementById('stat-ground');
const aircraftList = document.getElementById('aircraft-list');

// Telemetry Panel Elements
const panel = document.getElementById('telemetry-panel');
const btnClosePanel = document.getElementById('close-panel');
const pIcao = document.getElementById('panel-icao');
const pAlt = document.getElementById('panel-alt');
const pVel = document.getElementById('panel-vel');
const pHdg = document.getElementById('panel-hdg');
const pCoord = document.getElementById('panel-coord');
const pModel = document.getElementById('panel-model');
const pOperator = document.getElementById('panel-operator');

// Modal Elements
const modal = document.getElementById('info-modal');
const btnInfoPanel = document.getElementById('info-panel');
const btnCloseModal = document.getElementById('close-modal');

const mReg = document.getElementById('modal-reg');
const mFab = document.getElementById('m-fab');
const mMod = document.getElementById('m-mod');
const mSer = document.getElementById('m-ser');
const mCfg = document.getElementById('m-cfg');
const mCat = document.getElementById('m-cat');
const mAno = document.getElementById('m-ano');
const mIcao = document.getElementById('m-icao');
const mHab = document.getElementById('m-hab');
const mCls = document.getElementById('m-cls');
const mTrip = document.getElementById('m-trip');
const mPax = document.getElementById('m-pax');
const mAst = document.getElementById('m-ast');
const mPmd = document.getElementById('m-pmd');
const mSit = document.getElementById('m-sit');
const mCva = document.getElementById('m-cva');
const mRes = document.getElementById('m-res');
const mGrav = document.getElementById('m-grav');
const mTpop = document.getElementById('m-tpop');
const mTpca = document.getElementById('m-tpca');
const mProp = document.getElementById('m-prop');

const mOpNome = document.getElementById('m-op-nome');
const mOpDoc = document.getElementById('m-op-doc');
const mPrNome = document.getElementById('m-pr-nome');
const mPrDoc = document.getElementById('m-pr-doc');

const mAdsbCallsign = document.getElementById('m-adsb-callsign');
const mAdsbVersion = document.getElementById('m-adsb-version');
const mAdsbCategory = document.getElementById('m-adsb-category');

// Tabs
const tabBtns = document.querySelectorAll('.modal-tab-btn');
const tabContents = document.querySelectorAll('.modal-tab-content');

tabBtns.forEach(btn => {
    btn.addEventListener('click', () => {
        tabBtns.forEach(b => b.classList.remove('active'));
        tabContents.forEach(c => c.classList.remove('active'));
        btn.classList.add('active');
        document.getElementById(btn.dataset.tab).classList.add('active');
    });
});

let activeAcData = null;

btnInfoPanel.addEventListener('click', () => {
    if (!activeAcData) return;
    modal.classList.remove('hidden');
    mReg.textContent = activeAcData.registration !== 'UNKNOWN' ? activeAcData.registration : activeAcData.icao;
    
    if (activeAcData.anac) {
        const d = activeAcData.anac;
        mFab.textContent = d.fabricante || '---';
        mMod.textContent = d.modelo || '---';
        mSer.textContent = d.serial || '---';
        mCfg.textContent = d.config || '---';
        mCat.textContent = d.categoria || '---';
        mAno.textContent = d.ano || '---';
        mIcao.textContent = d.icao_type || '---';
        
        mHab.textContent = d.habilitacao || '---';
        mCls.textContent = d.classe || '---';
        mTrip.textContent = d.tripulacao || '---';
        mPax.textContent = d.pax || '---';
        mAst.textContent = d.assentos || '---';
        mPmd.textContent = (d.pmd && d.pmd !== 'N/A' && d.pmd !== 'nan') ? `${d.pmd} - Kg` : '---';
        
        mSit.textContent = d.situacao || '---';
        mCva.textContent = d.validade_cva || '---';
        mRes.textContent = d.restricao || '---';
        
        mGrav.textContent = d.gravame || '---';
        mTpop.textContent = d.operador_tipo || '---';
        mTpca.textContent = d.ca_tipo || '---';
        mProp.textContent = d.proposito || '---';
        
        mOpNome.textContent = d.operador_nome || '---';
        mOpDoc.textContent = d.operador_doc || '---';
        mPrNome.textContent = d.proprietario_nome || '---';
        mPrDoc.textContent = d.proprietario_doc || '---';
    } else {
        mFab.textContent = '---';
        mMod.textContent = '---';
        mSer.textContent = '---';
        mCfg.textContent = '---';
        mCat.textContent = '---';
        mAno.textContent = '---';
        mIcao.textContent = '---';
        mHab.textContent = '---';
        mCls.textContent = '---';
        mTrip.textContent = '---';
        mPax.textContent = '---';
        mAst.textContent = '---';
        mPmd.textContent = '---';
        mSit.textContent = '---';
        mCva.textContent = '---';
        mRes.textContent = '---';
        mGrav.textContent = '---';
        mTpop.textContent = '---';
        mTpca.textContent = '---';
        mProp.textContent = '---';
        mOpNome.textContent = '---';
        mOpDoc.textContent = '---';
        mPrNome.textContent = '---';
        mPrDoc.textContent = '---';
    }
    
    // ADS-B Live Data
    mAdsbCallsign.textContent = activeAcData.telemetry_callsign || 'Waiting...';
    mAdsbVersion.textContent = activeAcData.telemetry_version !== undefined ? activeAcData.telemetry_version : 'Waiting...';
    mAdsbCategory.textContent = activeAcData.telemetry_category || 'Waiting...';
    
    document.getElementById('info-modal').classList.add('active');
});

btnCloseModal.addEventListener('click', () => {
    modal.classList.add('hidden');
    document.getElementById('info-modal').classList.remove('active');
});

// Close panel logic
btnClosePanel.addEventListener('click', () => {
    panel.classList.add('hidden');
    if (selectedIcao && flightPaths[selectedIcao]) {
        map.removeLayer(flightPaths[selectedIcao].polyline);
    }
    selectedIcao = null;
});

function selectAircraft(icao, lat, lon) {
    if (selectedIcao && flightPaths[selectedIcao]) {
        map.removeLayer(flightPaths[selectedIcao].polyline);
    }
    selectedIcao = icao;
    if (flightPaths[selectedIcao]) {
        flightPaths[selectedIcao].polyline.addTo(map);
    }
    if (lat && lon) {
        map.flyTo([lat, lon], map.getZoom(), { animate: true, duration: 0.5 });
    }
}

// Update the list in sidebar
function renderList(data) {
    aircraftList.innerHTML = '';
    let airborneCount = 0;
    let groundCount = 0;

    // Sort data: Airborne first, then ground, then by ICAO
    const sorted = [...data].sort((a, b) => {
        if (a.is_surface !== b.is_surface) return a.is_surface ? 1 : -1;
        return a.icao.localeCompare(b.icao);
    });

    sorted.forEach(ac => {
        if (ac.is_surface) groundCount++;
        else airborneCount++;

        const li = document.createElement('li');
        li.className = 'aircraft-item';
        
        const typeClass = ac.is_surface ? 'type-ground' : 'type-airborne';
        const typeText = ac.category === 'GROUND_VEHICLE' ? 'VEHICLE' : (ac.is_surface ? 'GROUND' : 'AIRBORNE');
        
        li.innerHTML = `
            <span class="item-icao">${ac.registration !== 'UNKNOWN' ? ac.registration : ac.icao}</span>
            <span class="item-type ${typeClass}">${typeText}</span>
        `;
        
        li.addEventListener('click', () => {
            selectAircraft(ac.icao, ac.lat, ac.lon);
            updatePanel(ac);
        });

        aircraftList.appendChild(li);
    });

    statAirborne.textContent = airborneCount;
    statGround.textContent = groundCount;
}

// Update Map Markers
function renderMap(data) {
    // Keep track of which ICAOs we've seen in this fetch
    const currentIcaos = new Set();

    data.forEach(ac => {
        currentIcaos.add(ac.icao);
        
        const lat = ac.lat;
        const lon = ac.lon;
        const heading = ac.heading !== null ? ac.heading : 0;
        
        // Flight Path history
        if (!flightPaths[ac.icao]) {
            const color = ac.is_surface ? '#ff9100' : '#00e1ff';
            flightPaths[ac.icao] = {
                latlngs: [[lat, lon]],
                polyline: L.polyline([[lat, lon]], {color: color, weight: 3, opacity: 0.6, dashArray: '4, 6'})
            };
            if (selectedIcao === ac.icao) {
                flightPaths[ac.icao].polyline.addTo(map);
            }
        } else {
            const path = flightPaths[ac.icao];
            const lastPos = path.latlngs[path.latlngs.length - 1];
            if (lastPos[0] !== lat || lastPos[1] !== lon) {
                path.latlngs.push([lat, lon]);
                if (path.latlngs.length > 300) path.latlngs.shift(); // keep last 300 points
                path.polyline.setLatLngs(path.latlngs);
            }
        }
        
        if (!markers[ac.icao]) {
            // Create new marker
            const isGround = ac.is_surface;
            const isVehicle = ac.category === 'GROUND_VEHICLE';
            const markerClass = isGround ? 'aircraft-marker-ground' : 'aircraft-marker-airborne';
            const iconType = isVehicle ? 'fa-car' : 'fa-plane-up';
            const iconHtml = `<i class="fa-solid ${iconType} ${markerClass}" style="transform: rotate(${isVehicle ? heading - 90 : heading}deg);"></i>`;
                
            const customIcon = L.divIcon({
                html: iconHtml,
                className: 'custom-leaflet-icon',
                iconSize: [24, 24],
                iconAnchor: [12, 12]
            });

            const marker = L.marker([lat, lon], { icon: customIcon }).addTo(map);
            
            marker.on('click', () => {
                selectAircraft(ac.icao, null, null);
                updatePanel(ac);
            });
            
            markers[ac.icao] = marker;
        } else {
            // Update existing marker position
            const marker = markers[ac.icao];
            marker.setLatLng([lat, lon]);
            
            // Update rotation
            const isGround = ac.is_surface;
            const isVehicle = ac.category === 'GROUND_VEHICLE';
            const markerClass = isGround ? 'aircraft-marker-ground' : 'aircraft-marker-airborne';
            const iconType = isVehicle ? 'fa-car' : 'fa-plane-up';
            const iconHtml = `<i class="fa-solid ${iconType} ${markerClass}" style="transform: rotate(${isVehicle ? heading - 90 : heading}deg);"></i>`;
            
            marker.setIcon(L.divIcon({
                html: iconHtml,
                className: 'custom-leaflet-icon',
                iconSize: [24, 24],
                iconAnchor: [12, 12]
            }));
        }

        // If this is the currently selected aircraft, update the panel live
        if (selectedIcao === ac.icao) {
            updatePanel(ac);
        }
        
        // Also update modal live data if it's the active one
        if (activeAcData && activeAcData.icao === ac.icao) {
            activeAcData = ac; // Keep ref updated
            if (document.getElementById('info-modal').classList.contains('active')) {
                mAdsbCallsign.textContent = ac.telemetry_callsign || 'Waiting...';
                mAdsbVersion.textContent = ac.telemetry_version !== undefined ? ac.telemetry_version : 'Waiting...';
                mAdsbCategory.textContent = ac.telemetry_category || 'Waiting...';
            }
        }
    });

    // Remove old markers and flight paths
    Object.keys(markers).forEach(icao => {
        if (!currentIcaos.has(icao)) {
            map.removeLayer(markers[icao]);
            delete markers[icao];
            
            if (flightPaths[icao]) {
                map.removeLayer(flightPaths[icao].polyline);
                delete flightPaths[icao];
            }
            
            if (selectedIcao === icao) {
                panel.classList.add('hidden');
                selectedIcao = null;
            }
        }
    });
}

function updatePanel(ac) {
    activeAcData = ac;
    panel.classList.remove('hidden');
    
    const isVehicle = ac.category === 'GROUND_VEHICLE';
    document.getElementById('panel-icon-model').className = isVehicle ? 'fa-solid fa-car' : 'fa-solid fa-plane';
    document.getElementById('panel-label-model').textContent = isVehicle ? 'Vehicle Model' : 'Aircraft Model';
    
    pIcao.textContent = ac.registration !== 'UNKNOWN' ? ac.registration : ac.icao;
    pModel.textContent = ac.model || '---';
    pOperator.textContent = ac.operator || '---';
    pAlt.textContent = ac.alt !== null ? `${ac.alt} ft` : '---';
    pVel.textContent = ac.speed !== null ? `${ac.speed} kt` : '---';
    pHdg.textContent = ac.heading !== null ? `${ac.heading.toFixed(1)}°` : '---';
    pCoord.textContent = `${ac.lat.toFixed(4)}, ${ac.lon.toFixed(4)}`;
}

// Fetch loop
async function fetchTelemetry() {
    try {
        const response = await fetch('/api/aircraft');
        if (response.ok) {
            const data = await response.json();
            renderList(data);
            renderMap(data);
        }
    } catch (e) {
        console.error("Error fetching telemetry", e);
    }
}

// Start polling every second
setInterval(fetchTelemetry, 1000);
fetchTelemetry();
