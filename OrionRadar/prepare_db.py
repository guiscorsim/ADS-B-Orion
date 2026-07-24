import pandas as pd
import json
import base64
import csv

df = pd.read_csv('dados_aeronaves.csv', sep=';', encoding='latin1', skiprows=1, low_memory=False)
df = df.dropna(subset=['MARCAS', 'NM_FABRICANTE', 'DS_MODELO', 'OPERADORES'])
sample_planes = df.sample(15)

out_rows = []
out_rows.append(['icao24','registration','manufacturer','model','operator','year','engine','category','serial_number','max_pax','status','validity','anac_b64'])

def get_op(row):
    try:
        ops = json.loads(row['OPERADORES'])
        return ops[0]['NOME'] if len(ops) > 0 else 'UNKNOWN'
    except:
        return 'UNKNOWN'

def safe_int_str(val):
    if pd.isna(val) or val == '': return 'N/A'
    try: return str(int(float(val)))
    except: return str(val)

def build_anac_json(row):
    try: ops = json.loads(row['OPERADORES'])
    except: ops = []
    try: props = json.loads(row['PROPRIETARIOS'])
    except: props = []
    
    op_doc = ops[0]['DOCUMENTO'] if len(ops) > 0 and 'DOCUMENTO' in ops[0] else 'N/A'
    prop_nome = props[0]['NOME'] if len(props) > 0 and 'NOME' in props[0] else 'N/A'
    prop_doc = props[0]['DOCUMENTO'] if len(props) > 0 and 'DOCUMENTO' in props[0] else 'N/A'
    
    classe = f"{row['TP_POUSO']} {safe_int_str(row['QT_MOTOR'])} {row['TP_MOTOR']}"
    
    data = {
        'fabricante': row['NM_FABRICANTE'],
        'modelo': row['DS_MODELO'],
        'serial': str(row['NR_SERIE']),
        'config': str(row['CF_OPERACIONAL']),
        'categoria': str(row['DS_CATEGORIA_HOMOLOGACAO']),
        'ano': safe_int_str(row['NR_ANO_FABRICACAO']),
        'icao_type': str(row['CD_TIPO_ICAO']),
        'habilitacao': str(row['CD_TIPO']),
        'classe': classe,
        'tripulacao': safe_int_str(row['NR_TRIPULACAO_MIN']),
        'pax': safe_int_str(row['NR_PASSAGEIROS_MAX']),
        'assentos': safe_int_str(row['NR_ASSENTOS']),
        'pmd': str(row['NR_PMD']),
        'situacao': 'MATRÍCULA CANCELADA' if pd.notna(row['DT_CANC']) else 'SITUAÇÃO NORMAL',
        'restricao': str(row['DS_MOTIVO_CANC']) if pd.notna(row['DS_MOTIVO_CANC']) else 'N/A',
        'gravame': str(row['DS_GRAVAME']),
        'validade_cva': str(row['DT_VALIDADE_CVA']),
        'operador_tipo': str(row['TP_OPERACAO']),
        'ca_tipo': str(row['TP_CA']),
        'proposito': str(row['CD_PROPOSITO_CAVE']) if pd.notna(row['CD_PROPOSITO_CAVE']) else 'N/A',
        'operador_nome': get_op(row),
        'operador_doc': op_doc,
        'proprietario_nome': prop_nome,
        'proprietario_doc': prop_doc
    }
    j_str = json.dumps(data)
    return base64.b64encode(j_str.encode('utf-8')).decode('utf-8')

for i in range(10):
    row = sample_planes.iloc[i]
    icao24 = hex(0xE00000 + i)[2:].upper()
    reg = row['MARCAS']
    if len(reg) == 5: reg = f"{reg[:2]}-{reg[2:]}"
    out_rows.append([icao24, reg, row['NM_FABRICANTE'], row['DS_MODELO'], get_op(row), safe_int_str(row['NR_ANO_FABRICACAO']), row['TP_MOTOR'], 'AIRBORNE', row['NR_SERIE'], safe_int_str(row['NR_PASSAGEIROS_MAX']), row['DS_GRAVAME'], row['DT_VALIDADE_CVA'], build_anac_json(row)])

for i in range(10, 15):
    row = sample_planes.iloc[i]
    icao24 = hex(0xE00000 + i)[2:].upper()
    reg = row['MARCAS']
    if len(reg) == 5: reg = f"{reg[:2]}-{reg[2:]}"
    out_rows.append([icao24, reg, row['NM_FABRICANTE'], row['DS_MODELO'], get_op(row), safe_int_str(row['NR_ANO_FABRICACAO']), row['TP_MOTOR'], 'GROUND_AIRCRAFT', row['NR_SERIE'], safe_int_str(row['NR_PASSAGEIROS_MAX']), row['DS_GRAVAME'], row['DT_VALIDADE_CVA'], build_anac_json(row)])

vehicles = [
    ('CGH-01', 'Volkswagen', 'Saveiro', 'Infraero', '2015', 'MOTOR CONVENCIONAL', 'BR-01', '2', 'ATIVO', 'N/A'),
    ('CGH-02', 'Mercedes', 'Sprinter', 'Infraero', '2018', 'MOTOR CONVENCIONAL', 'BR-02', '15', 'ATIVO', 'N/A'),
    ('CGH-03', 'Volvo', 'B290R', 'LATAM Ground', '2019', 'MOTOR CONVENCIONAL', 'BR-03', '60', 'ATIVO', 'N/A'),
    ('CGH-04', 'Toyota', 'Hilux', 'Azul Ground', '2021', 'MOTOR CONVENCIONAL', 'BR-04', '5', 'ATIVO', 'N/A'),
    ('CGH-05', 'Volkswagen', 'Kombi', 'GOL Ground', '2010', 'MOTOR CONVENCIONAL', 'BR-05', '9', 'ATIVO', 'N/A')
]

for i in range(5):
    icao24 = hex(0xF00000 + i)[2:].upper()
    v = vehicles[i]
    
    vehicle_anac = {
        'fabricante': v[1],
        'modelo': v[2],
        'serial': v[6],
        'config': 'VEÍCULO DE APOIO',
        'categoria': 'TERRESTRE',
        'ano': v[4],
        'icao_type': 'VEH',
        'habilitacao': 'CNH',
        'classe': 'AUTOMOTIVO',
        'tripulacao': '1',
        'pax': v[7],
        'assentos': str(int(v[7]) + 1),
        'pmd': 'N/A',
        'situacao': v[8],
        'restricao': 'ÁREA RESTRITA (AR)',
        'gravame': 'NENHUM',
        'validade_cva': 'N/A',
        'operador_tipo': 'AEROPORTUÁRIO',
        'ca_tipo': 'N/A',
        'proposito': 'APOIO DE SOLO',
        'operador_nome': v[3],
        'operador_doc': 'N/A',
        'proprietario_nome': v[3],
        'proprietario_doc': 'N/A'
    }
    
    v_b64 = base64.b64encode(json.dumps(vehicle_anac).encode('utf-8')).decode('utf-8')
    out_rows.append([icao24, v[0], v[1], v[2], v[3], v[4], v[5], 'GROUND_VEHICLE', v[6], v[7], v[8], v[9], v_b64])

with open('aircraft_database.csv', 'w', encoding='utf-8', newline='') as f:
    writer = csv.writer(f)
    writer.writerows(out_rows)
